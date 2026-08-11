import { mkdir, readFile, writeFile } from "node:fs/promises"
import path from "node:path"

const BEGIN_USE = ["oris_experience_begin_use", "oris.experience.begin_use"]
const RECORD_OUTCOME = ["oris_experience_record_outcome", "oris.experience.record_outcome"]

function matches(tool, names) {
  return names.some((name) => tool.endsWith(name))
}

function sessionID(value) {
  return value?.sessionID ?? value?.sessionId ?? value?.properties?.sessionID ?? value?.properties?.info?.id ?? "unknown"
}

export const OrisExperiencePlugin = async ({ directory, worktree }) => {
  const root = worktree || directory
  const stateDir = path.join(root, ".oris", "agent-usage")
  const pendingCalls = new Map()
  const activeRuns = new Map()

  async function save(session, state) {
    await mkdir(stateDir, { recursive: true })
    const target = path.join(stateDir, `opencode-${String(session).replaceAll("/", "_")}.json`)
    await writeFile(target, JSON.stringify(state, null, 2))
  }

  async function markUnfinished(session) {
    const active = activeRuns.get(session)
    if (!active) return
    let previous = active
    try {
      previous = JSON.parse(await readFile(active.path, "utf8"))
    } catch {}
    if (previous.status === "active") {
      const next = {
        ...previous,
        status: "pending_inconclusive_receipt",
        reason: "OpenCode session ended without an evidence-backed record_outcome call",
      }
      await save(session, next)
    }
  }

  return {
    "tool.execute.before": async (input, output) => {
      const tool = String(input.tool || "")
      if (!matches(tool, BEGIN_USE) && !matches(tool, RECORD_OUTCOME)) return
      const key = `${sessionID(input)}:${input.callID || input.callId || tool}`
      pendingCalls.set(key, { tool, args: output.args || {} })
    },
    "tool.execute.after": async (input) => {
      const session = sessionID(input)
      const key = `${session}:${input.callID || input.callId || input.tool}`
      const call = pendingCalls.get(key)
      pendingCalls.delete(key)
      if (!call) return
      if (matches(call.tool, BEGIN_USE)) {
        const state = { status: "active", arguments: call.args }
        await save(session, state)
        activeRuns.set(session, { path: path.join(stateDir, `opencode-${String(session).replaceAll("/", "_")}.json`) })
      } else if (matches(call.tool, RECORD_OUTCOME)) {
        await save(session, { status: "recorded", arguments: call.args })
        activeRuns.delete(session)
      }
    },
    event: async ({ event }) => {
      if (event.type === "session.idle" || event.type === "session.deleted" || event.type === "session.error") {
        await markUnfinished(sessionID(event))
      }
    },
  }
}
