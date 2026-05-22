#!/usr/bin/env bash
# Runs as a Claude Code Stop hook.
# Reads the session transcript, asks claude -p to identify any decisions worth
# logging, and appends suggestions to docs/decisions_pending.md for review.

GOLDFISH_DIR="/Users/felixbaileymurray/Documents/goldfish"

# Guard: only run when inside this project
case "${PWD:-}" in
    "$GOLDFISH_DIR"|"$GOLDFISH_DIR"/*) ;;
    *) exit 0 ;;
esac

# Read hook payload and extract session_id
PAYLOAD=$(cat)
SESSION_ID=$(echo "$PAYLOAD" | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    print(d.get('session_id', ''))
except:
    pass
" 2>/dev/null)

[[ -n "$SESSION_ID" ]] || exit 0

TRANSCRIPT_FILE="$HOME/.claude/projects/-Users-felixbaileymurray-Documents-goldfish/${SESSION_ID}.jsonl"
[[ -f "$TRANSCRIPT_FILE" ]] || exit 0

DECISIONS_FILE="${GOLDFISH_DIR}/docs/decisions.md"
PENDING_FILE="${GOLDFISH_DIR}/docs/decisions_pending.md"

# Extract human-readable conversation from the transcript.
# Exits non-zero (skipping analysis) if fewer than 2 user messages found.
CONV=$(python3 - "$TRANSCRIPT_FILE" << 'PYEOF'
import sys, json

transcript = sys.argv[1]
messages = []
user_count = 0

with open(transcript) as f:
    for line in f:
        try:
            obj = json.loads(line)
            t = obj.get('type')
            if t not in ('user', 'assistant'):
                continue
            msg = obj.get('message', {})
            role = msg.get('role', t)
            content = msg.get('content', '')
            if isinstance(content, list):
                text = ' '.join(
                    p.get('text', '')
                    for p in content
                    if isinstance(p, dict) and p.get('type') == 'text'
                )
            else:
                text = str(content)
            text = text.strip()
            if not text:
                continue
            if role == 'user':
                user_count += 1
            # Truncate individual messages to keep the prompt manageable
            messages.append(f"[{role.upper()}]: {text[:600]}")
        except Exception:
            pass

# Skip analysis for very short / incomplete sessions
if user_count < 2:
    sys.exit(1)

# Send only the last 80 messages to stay within token limits
print('\n\n'.join(messages[-80:]))
PYEOF
) || exit 0

[[ -n "$CONV" ]] || exit 0

EXISTING=$(cat "$DECISIONS_FILE" 2>/dev/null || true)
PENDING=$(cat "$PENDING_FILE" 2>/dev/null || true)
TODAY=$(date +%Y-%m-%d)

PROMPT="You are reviewing a Claude Code session for the Goldfish project to identify decisions worth logging.

CURRENT DECISIONS LOG (docs/decisions.md):
${EXISTING}

ALREADY PENDING SUGGESTIONS (docs/decisions_pending.md — do not re-suggest anything already here):
${PENDING}

CONVERSATION:
${CONV}

Identify decisions made in this conversation that are NOT already in either file above.

Worth logging: non-obvious architecture choices, product direction, technology selection, file layout, deferred work with a named trigger, rejected alternatives with lasting implications.
Not worth logging: routine bug fixes, implementation details, obvious choices, UI tweaks with no long-term impact.

For each new decision, use this exact format:
## ${TODAY} — Short title

**Status:** decided | proposed | superseded
**Context:** …
**Decision:** …
**Consequences:** …
**Alternatives considered:** …

---

If nothing new is worth logging, output exactly: NOTHING_TO_LOG

Output ONLY the formatted entries or NOTHING_TO_LOG — no preamble, no commentary."

SUGGESTIONS=$(claude -p "$PROMPT" 2>/dev/null || true)

if [[ -n "$SUGGESTIONS" && "$SUGGESTIONS" != "NOTHING_TO_LOG" ]]; then
    {
        printf '\n<!-- Session %s — %s — review, edit, then move entries to decisions.md -->\n\n' \
            "${SESSION_ID:0:8}" "$(date '+%Y-%m-%d %H:%M')"
        printf '%s\n' "$SUGGESTIONS"
    } >> "$PENDING_FILE"
fi
