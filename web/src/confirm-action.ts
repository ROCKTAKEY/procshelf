import type { Action } from "./protocol";

export type Confirmation = {
  title: string;
  effects: string[];
  sequence: boolean;
  label: string;
};

export async function confirmAction(
  action: Action,
  name: string,
  ask: (confirmation: Confirmation) => boolean | Promise<boolean>,
): Promise<boolean> {
  if (action === "start") return true;
  const operations = {
    stop: { verb: "停止", effects: ["配下のプロセスも停止します。"] },
    restart: {
      verb: "再起動",
      effects: ["配下のプロセスも停止", "設定に従って起動"],
    },
    kill: {
      verb: "強制終了",
      effects: [
        "配下のプロセスも強制終了します。",
        "終了処理を待たずに停止します。",
      ],
    },
  };
  const { verb, effects } = operations[action];
  return ask({
    title: `${name} を${verb}しますか？`,
    effects,
    sequence: action === "restart",
    label: `${verb}する`,
  });
}
