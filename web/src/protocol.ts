import { z } from "zod";

export const processSchema = z.object({
  id: z.string(),
  name: z.string(),
  description: z.string(),
  state: z.enum([
    "stopped",
    "starting",
    "running",
    "stopping",
    "exited",
    "failed",
    "unknown",
  ]),
  run_id: z.number().int().nullable(),
  pid: z.number().int().nullable(),
  autostart: z.boolean(),
  config_pending: z.boolean(),
  removed: z.boolean(),
  outcome: z.string().nullable(),
  program: z.string(),
  args: z.array(z.string()),
  cwd: z.string(),
});
export const snapshotSchema = z.object({
  revision: z.number().int(),
  config_generation: z.number().int(),
  config_error: z.string().nullable(),
  listen: z.string(),
  configured_listen: z.string(),
  shutting_down: z.boolean(),
  processes: z.array(processSchema),
});
export const logsSchema = z.object({
  discarded: z.number().int(),
  entries: z.array(
    z.object({
      seq: z.number().int(),
      run_id: z.number().int(),
      time_ms: z.number(),
      stream: z.string(),
      text: z.string(),
    }),
  ),
});
export const sessionSchema = z.object({ token: z.string().min(32) });
export type Snapshot = z.infer<typeof snapshotSchema>;
export type Process = z.infer<typeof processSchema>;
export type Logs = z.infer<typeof logsSchema>;
export type Action = "start" | "stop" | "kill" | "restart";
export function canStart(state: string, removed: boolean): boolean {
  return !removed && ["stopped", "exited", "failed"].includes(state);
}
export const stateLabels: Record<Process["state"], string> = {
  stopped: "停止中",
  starting: "起動中",
  running: "稼働中",
  stopping: "停止処理中",
  exited: "終了",
  failed: "起動失敗",
  unknown: "状態不明",
};
