use std::collections::HashMap;

use base64::prelude::{Engine as _, BASE64_STANDARD};
use serde_json::{json, Value};
use wasmi::{Config, Engine, Linker, Module, Store, StoreLimitsBuilder, Val};

// 不导入 WASI、文件、网络或宿主函数。插件描述导出调用和内存操作，宿主只执行有界计算。
pub(super) fn execute(params: &Value) -> Result<Value, String> {
    let encoded = text(params, "module_base64")?;
    if encoded.len() > 4 * 1024 * 1024 {
        return Err(failure("WASM 模块超过 3 MiB"));
    }
    let bytes = BASE64_STANDARD.decode(encoded).map_err(failure)?;
    let operations = params["operations"]
        .as_array()
        .filter(|items| items.len() <= 128)
        .ok_or_else(|| failure("WASM 操作必须是最多 128 项的数组"))?;
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &bytes[..]).map_err(failure)?;
    let limits = StoreLimitsBuilder::new()
        .memory_size(64 * 1024 * 1024)
        .memories(1)
        .instances(1)
        .tables(4)
        .table_elements(100_000)
        .build();
    let mut store = Store::new(&engine, limits);
    store.limiter(|limits| limits);
    store
        .set_fuel(
            params["fuel"]
                .as_u64()
                .unwrap_or(2_000_000_000)
                .min(10_000_000_000),
        )
        .map_err(failure)?;
    let instance = Linker::new(&engine)
        .instantiate(&mut store, &module)
        .and_then(|instance| instance.start(&mut store))
        .map_err(failure)?;
    let mut results = HashMap::<String, Value>::new();
    for operation in operations {
        let value = match text(operation, "op")? {
            "call" => {
                let function = instance
                    .get_func(&store, text(operation, "export")?)
                    .ok_or_else(|| failure("WASM 导出函数不存在"))?;
                let signature = function.ty(&store);
                let arguments = operation["args"]
                    .as_array()
                    .ok_or_else(|| failure("WASM 函数参数必须是数组"))?;
                if arguments.len() != signature.params().len() {
                    return Err(failure("WASM 函数参数数量不匹配"));
                }
                let inputs = arguments
                    .iter()
                    .zip(signature.params())
                    .map(|(value, kind)| {
                        let number = resolve(value, &results)?
                            .as_f64()
                            .ok_or_else(|| failure("WASM 参数必须为有限数字"))?;
                        Ok(match Val::default(*kind) {
                            Val::I32(_)
                                if number.fract() == 0.0
                                    && number >= i32::MIN as f64
                                    && number <= u32::MAX as f64 =>
                            {
                                Val::I32(number as i64 as i32)
                            }
                            Val::F64(_) => Val::F64(number.into()),
                            Val::F32(_) => Val::F32((number as f32).into()),
                            _ => return Err(failure("WASM 参数类型不受支持")),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let mut outputs = signature
                    .results()
                    .iter()
                    .map(|kind| Val::default(*kind))
                    .collect::<Vec<_>>();
                function
                    .call(&mut store, &inputs, &mut outputs)
                    .map_err(failure)?;
                match outputs.first() {
                    Some(Val::I32(value)) => json!(value),
                    Some(Val::F64(value)) => json!(f64::from(*value)),
                    Some(Val::F32(value)) => json!(f32::from(*value)),
                    None => Value::Null,
                    _ => return Err(failure("WASM 返回类型不受支持")),
                }
            }
            "write_utf8" | "read_i32" | "read_f64" => {
                let memory = instance
                    .get_memory(&store, "memory")
                    .ok_or_else(|| failure("WASM 未导出 memory"))?;
                let offset = resolve(&operation["offset"], &results)?
                    .as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                    .ok_or_else(|| failure("WASM 内存偏移无效"))?;
                let offset = offset
                    .checked_add(operation["add"].as_u64().unwrap_or(0) as usize)
                    .ok_or_else(|| failure("WASM 内存偏移溢出"))?;
                match text(operation, "op")? {
                    "write_utf8" => {
                        let value = text(operation, "text")?;
                        if value.len() > 1024 * 1024 {
                            return Err(failure("WASM 内存写入超过 1 MiB"));
                        }
                        memory
                            .write(&mut store, offset, value.as_bytes())
                            .map_err(failure)?;
                        json!(value.len())
                    }
                    "read_i32" => {
                        let mut bytes = [0; 4];
                        memory.read(&store, offset, &mut bytes).map_err(failure)?;
                        json!(i32::from_le_bytes(bytes))
                    }
                    _ => {
                        let mut bytes = [0; 8];
                        memory.read(&store, offset, &mut bytes).map_err(failure)?;
                        let value = f64::from_le_bytes(bytes);
                        if !value.is_finite() {
                            return Err(failure("WASM 返回了非有限数字"));
                        }
                        json!(value)
                    }
                }
            }
            _ => return Err(failure("WASM 操作不受支持")),
        };
        if let Some(name) = operation["save_as"].as_str() {
            results.insert(name.to_string(), value);
        }
    }
    Ok(json!(results))
}

fn resolve(value: &Value, results: &HashMap<String, Value>) -> Result<Value, String> {
    if let Some(name) = value["result"].as_str() {
        return results
            .get(name)
            .cloned()
            .ok_or_else(|| failure("WASM 结果引用不存在"));
    }
    Ok(value.clone())
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| failure(format!("WASM 缺少 {key}")))
}

fn failure(error: impl std::fmt::Display) -> String {
    format!("WASM 执行失败: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_operations_are_bounded() {
        // 一个只导出一页 memory 的标准 WASM 模块，不含宿主导入。
        let module = BASE64_STANDARD.encode([
            0, 97, 115, 109, 1, 0, 0, 0, 5, 3, 1, 0, 1, 7, 10, 1, 6, 109, 101, 109, 111, 114, 121,
            2, 0,
        ]);
        let value = execute(&json!({"module_base64":module,"operations":[
            {"op":"write_utf8","offset":0,"text":"ABCD"},
            {"op":"read_i32","offset":0,"save_as":"word"}
        ]}))
        .unwrap();
        assert_eq!(value["word"], 0x44434241);
        assert!(execute(&json!({"module_base64":module,"operations":[
            {"op":"read_f64","offset":65535}
        ]}))
        .is_err());
        assert!(execute(&json!({"module_base64":module,"operations":[
            {"op":"read_i32","offset":{"result":"missing"}}
        ]}))
        .is_err());
    }

    #[test]
    fn unbounded_wasm_loop_exhausts_fuel() {
        let module = BASE64_STANDARD.encode([
            0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2, 1, 0, 7, 5, 1, 1, 102, 0, 0, 10,
            9, 1, 7, 0, 3, 64, 12, 0, 11, 11,
        ]);
        let error = execute(&json!({"module_base64":module,"fuel":1000,"operations":[
            {"op":"call","export":"f","args":[]}
        ]}))
        .unwrap_err();
        assert!(error.to_string().contains("fuel"));
    }
}
