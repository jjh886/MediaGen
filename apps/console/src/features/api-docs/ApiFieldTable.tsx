import type { ApiField } from './schema'

export default function ApiFieldTable({
  title,
  fields,
  response = false,
}: {
  title: string
  fields: ApiField[]
  response?: boolean
}) {
  return (
    <section className="api-docs__field-section">
      <h3>
        {title}
        <span>{fields.length} 个字段</span>
      </h3>
      <div className="api-docs__table-scroll" role="region" aria-label={title} tabIndex={0}>
        <table className="api-docs__field-table" aria-label={title}>
          <thead>
            <tr>
              <th scope="col">{response ? '字段名' : '参数名'}</th>
              <th scope="col">类型</th>
              <th scope="col">{response ? '是否必返 / 条件' : '是否必填'}</th>
              <th scope="col">{response ? '固定值 / 空值' : '默认值'}</th>
              <th scope="col">说明</th>
            </tr>
          </thead>
          <tbody>
            {fields.map((field) => (
              <tr key={`${field.location}-${field.name}`}>
                <td>
                  <code>{field.name}</code>
                </td>
                <td>
                  <code className="api-docs__field-type">{field.type}</code>
                </td>
                <td>
                  <span
                    className={
                      field.required === '是' ? 'api-docs__required' : 'api-docs__conditional'
                    }
                  >
                    {field.required}
                  </span>
                </td>
                <td>
                  <code>{field.defaultValue}</code>
                </td>
                <td>{field.description}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  )
}
