#!/usr/bin/env bash
# AgentBus UserPromptSubmit hook — injects inbox messages as context
# Install to: <project>/.claude/hooks/ (via agent-bus install-hooks)
# Fires before LLM sees user message. Silent when inbox is empty.
# Silent failure throughout — never generates hook errors.
set +e

PROJECT_NAME="${AGENT_BUS_PROJECT:-$(basename "$PWD")}"
INBOX_DIR="$HOME/.agent-bus/inbox/$PROJECT_NAME"
PROCESSED_DIR="$HOME/.agent-bus/processed"
BUS_URL="http://localhost:8090/api/bus"

# Exit silently if no inbox directory or no files
[ -d "$INBOX_DIR" ] || exit 0

# Collect JSON files
shopt -s nullglob
files=("$INBOX_DIR"/*.json)
shopt -u nullglob

[ ${#files[@]} -eq 0 ] && exit 0

# Ensure processed directory exists
mkdir -p "$PROCESSED_DIR"

# Sort files by priority (DESC) then created_at (ASC)
# Build sortable lines: "priority|created_at|filepath" then sort
sorted_files=()
while IFS= read -r line; do
  sorted_files+=("${line##*|}")
done < <(
  for f in "${files[@]}"; do
    pri=$(python3 -c "import json,sys; d=json.load(open(sys.argv[1])); print(d.get('priority',0))" "$f" 2>/dev/null || echo "0")
    cat=$(python3 -c "import json,sys; d=json.load(open(sys.argv[1])); print(d.get('created_at',''))" "$f" 2>/dev/null || echo "")
    # Negate priority for descending sort, then ascending created_at
    neg_pri=$(( 999 - pri ))
    printf '%03d|%s|%s\n' "$neg_pri" "$cat" "$f"
  done | sort
)

[ ${#sorted_files[@]} -eq 0 ] && exit 0

# Print header
echo "[AgentBus inbox — ${#sorted_files[@]} pending message$([ ${#sorted_files[@]} -gt 1 ] && echo "s")]"
echo ""

# Process each message
for f in "${sorted_files[@]}"; do
  # Extract fields
  msg_id=$(python3 -c "import json,sys; d=json.load(open(sys.argv[1])); print(d.get('id','?'))" "$f" 2>/dev/null)
  from=$(python3 -c "import json,sys; d=json.load(open(sys.argv[1])); print(d.get('from','unknown'))" "$f" 2>/dev/null)
  pri=$(python3 -c "import json,sys; d=json.load(open(sys.argv[1])); print(d.get('priority',0))" "$f" 2>/dev/null)
  content=$(python3 -c "import json,sys; d=json.load(open(sys.argv[1])); print(d.get('content',''))" "$f" 2>/dev/null)

  echo "--- Message from $from (priority: $pri) ---"
  echo "$content"
  echo ""

  # Move file to processed BEFORE acking (atomic move, ACK can fail)
  mv "$f" "$PROCESSED_DIR/" 2>/dev/null

  # POST ACK to bus (silent failure OK)
  curl -s -X POST "$BUS_URL/ack" \
    -H "Content-Type: application/json" \
    -d "{\"message_id\": $msg_id, \"handled_by\": \"claude_code\"}" \
    > /dev/null 2>&1 || true
done
