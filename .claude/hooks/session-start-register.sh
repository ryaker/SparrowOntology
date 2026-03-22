#!/usr/bin/env bash
# AgentBus SessionStart hook — registers this session with the bus
# Install to: <project>/.claude/hooks/ (via agent-bus install-hooks)
# Fires at session start. Silent failure — bus may not be running.

PROJECT_NAME="${AGENT_BUS_PROJECT:-$(basename "$PWD")}"
BUS_URL="http://localhost:8090/api/bus"

curl -s -X POST "$BUS_URL/register" \
  -H "Content-Type: application/json" \
  -d "{
    \"project\": \"$PROJECT_NAME\",
    \"folder_path\": \"$PWD\",
    \"runtime\": \"claude_code\",
    \"pid\": $PPID
  }" > /dev/null 2>&1 || true
