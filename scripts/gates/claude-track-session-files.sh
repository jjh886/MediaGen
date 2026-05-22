#!/bin/bash
# Claude Code PostToolUse hook：追踪当前会话修改的文件
#
# 在 Write / Edit 工具执行后，将修改的文件路径记录到会话级临时文件中。
# Stop hook 读取该文件来判断本次会话修改了哪些模块，避免多会话并行时互相干扰。

set -uo pipefail

# 从 stdin 读取 JSON（Claude Code 传入的 tool_input）
INPUT=$(cat)

# 提取 file_path（兼容 jq 不可用的情况）
if command -v jq &>/dev/null; then
  FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty' 2>/dev/null)
else
  # 简易提取：匹配 "file_path": "..." 模式
  FILE_PATH=$(echo "$INPUT" | sed -n 's/.*"file_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)
fi

# 无法提取文件路径则跳过
if [ -z "$FILE_PATH" ]; then
  exit 0
fi

# 会话标识：优先用 CLAUDE_SESSION_ID，其次用 PPID（Claude Code 进程 PID）
SESSION_KEY="${CLAUDE_SESSION_ID:-ppid_$PPID}"
TRACK_FILE="/tmp/claude-session-${SESSION_KEY}-files.txt"

# 追加文件路径（去重在 Stop hook 中处理）
echo "$FILE_PATH" >> "$TRACK_FILE"

exit 0
