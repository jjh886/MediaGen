#!/usr/bin/env node

/**
 * 轻量级结构复杂度检查。
 *
 * 目标不是替代 AST 级分析器，而是在开发启动前尽早拦住两类高风险代码：
 * 1. 单个函数过长，后续继续补丁式维护会越来越难看懂。
 * 2. 单个函数控制分支过多，review 和测试成本会明显上升。
 *
 * 这个脚本默认只对"当前改动文件"执行，避免历史存量问题一次性把开发链路全部卡死。
 */

import { promises as fs } from 'node:fs'
import path from 'node:path'

const DEFAULT_MAX_FUNCTION_LINES = 50
const DEFAULT_MAX_DECISIONS = 10
const SUPPORTED_EXTENSIONS = new Set(['.ts', '.tsx', '.js', '.jsx', '.go', '.rs'])

function parseArgs(argv) {
  let maxFunctionLines = DEFAULT_MAX_FUNCTION_LINES
  let maxDecisions = DEFAULT_MAX_DECISIONS
  const files = []

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--max-function-lines') {
      maxFunctionLines = Number(argv[index + 1] || DEFAULT_MAX_FUNCTION_LINES)
      index += 1
      continue
    }
    if (arg === '--max-decisions') {
      maxDecisions = Number(argv[index + 1] || DEFAULT_MAX_DECISIONS)
      index += 1
      continue
    }
    files.push(arg)
  }

  return { files, maxFunctionLines, maxDecisions }
}

function isSupportedFile(filePath) {
  return SUPPORTED_EXTENSIONS.has(path.extname(filePath))
}

function countBraceDelta(line) {
  const opens = (line.match(/\{/g) || []).length
  const closes = (line.match(/\}/g) || []).length
  return opens - closes
}

function lineLooksLikeFunctionStart(line) {
  return [
    /^\s*(export\s+)?(async\s+)?function\b/,
    /^\s*(export\s+)?(const|let|var)\s+[A-Za-z0-9_$]+\s*=\s*(async\s*)?\([^)]*\)\s*=>/,
    /^\s*(export\s+)?(const|let|var)\s+[A-Za-z0-9_$]+\s*=\s*(async\s*)?[A-Za-z0-9_$]+\s*=>/,
    /^\s*(public|private|protected|static|async|\s)*[A-Za-z0-9_$]+\s*\([^)]*\)\s*\{/,
    /^\s*func\b/,
    /^\s*(pub\s+)?(async\s+)?fn\b/,
  ].some((pattern) => pattern.test(line))
}

function lineLooksMeaningful(line) {
  const trimmed = line.trim()
  if (!trimmed) {
    return false
  }
  if (['{', '}', '};', '})', ');'].includes(trimmed)) {
    return false
  }
  if (trimmed.startsWith('//') || trimmed.startsWith('*') || trimmed.startsWith('/*')) {
    return false
  }
  return true
}

function countDecisionPoints(line) {
  const matches = line.match(/\b(if|else\s+if|switch|case|for|while|catch)\b|&&|\|\|/g)
  return matches ? matches.length : 0
}

function summarizeSignature(line) {
  return line.trim().replace(/\s+/g, ' ').slice(0, 120)
}

function inspectFunctions(content) {
  const lines = content.split(/\r?\n/)
  const findings = []

  let current = null
  let pendingSignature = null
  let pendingLineNumber = null

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    const lineNumber = index + 1

    if (!current && !pendingSignature && lineLooksLikeFunctionStart(line)) {
      pendingSignature = summarizeSignature(line)
      pendingLineNumber = lineNumber
      if (line.includes('{')) {
        current = {
          signature: pendingSignature,
          startLine: pendingLineNumber,
          braceDepth: countBraceDelta(line),
          meaningfulLines: lineLooksMeaningful(line) ? 1 : 0,
          decisions: countDecisionPoints(line),
        }
        pendingSignature = null
        pendingLineNumber = null
        if (current.braceDepth <= 0) {
          current = null
        }
      }
      continue
    }

    if (!current && pendingSignature) {
      if (!line.includes('{')) {
        continue
      }
      current = {
        signature: pendingSignature,
        startLine: pendingLineNumber,
        braceDepth: countBraceDelta(line),
        meaningfulLines: lineLooksMeaningful(line) ? 1 : 0,
        decisions: countDecisionPoints(line),
      }
      pendingSignature = null
      pendingLineNumber = null
      if (current.braceDepth <= 0) {
        current = null
      }
      continue
    }

    if (!current) {
      continue
    }

    current.braceDepth += countBraceDelta(line)
    if (lineLooksMeaningful(line)) {
      current.meaningfulLines += 1
    }
    current.decisions += countDecisionPoints(line)

    if (current.braceDepth <= 0) {
      findings.push({
        signature: current.signature,
        startLine: current.startLine,
        meaningfulLines: current.meaningfulLines,
        decisions: current.decisions,
      })
      current = null
    }
  }

  return findings
}

async function main() {
  const { files, maxFunctionLines, maxDecisions } = parseArgs(process.argv.slice(2))
  const supportedFiles = files.filter(isSupportedFile)

  if (supportedFiles.length === 0) {
    console.log('没有需要执行复杂度检查的改动文件。')
    return
  }

  const offenders = []
  for (const filePath of supportedFiles) {
    let content = ''
    try {
      content = await fs.readFile(filePath, 'utf8')
    } catch (error) {
      if (error && typeof error === 'object' && 'code' in error && error.code === 'ENOENT') {
        continue
      }
      throw error
    }
    const functions = inspectFunctions(content)

    for (const item of functions) {
      if (item.meaningfulLines > maxFunctionLines || item.decisions > maxDecisions) {
        offenders.push({
          filePath,
          ...item,
        })
      }
    }
  }

  if (offenders.length === 0) {
    console.log('结构复杂度检查通过。')
    return
  }

  console.error('\n发现复杂度偏高的函数，已阻断启动：\n')
  for (const offender of offenders) {
    console.error(
      `  ${offender.filePath}:${offender.startLine} ` +
        `函数行数=${offender.meaningfulLines} 决策点=${offender.decisions} ` +
        `=> ${offender.signature}`
    )
  }

  console.error(
    '\n建议：把过长函数拆成更小的步骤函数，把多分支判断下沉到 helper / service / adapter，不要继续往同一个函数里补 if/else。'
  )
  process.exitCode = 1
}

await main()
