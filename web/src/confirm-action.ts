import type { Action } from "./protocol";

export type Confirmation = {
  title: string;
  message: string;
  label: string;
};

export async function confirmAction(
  action: Action,
  name: string,
  ask: (confirmation: Confirmation) => boolean | Promise<boolean>,
): Promise<boolean> {
  if (action === "start") return true;
  const operations = {
    stop: { verb: "停止", message: "配下のプロセスも停止します。" },
    restart: {
      verb: "再起動",
      message: "配下のプロセスも停止してから、設定に従って起動し直します。",
    },
    kill: {
      verb: "強制終了",
      message: "配下のプロセスも強制終了します。終了処理を待たずに停止します。",
    },
  };
  const { verb, message } = operations[action];
  return ask({
    title: `${name} を${verb}しますか？`,
    message,
    label: `${verb}する`,
  });
}
