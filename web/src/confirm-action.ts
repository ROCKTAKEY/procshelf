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
    stop: { verb: "停止", effects: [`${name} とその子プロセスを停止します。`] },
    restart: {
      verb: "再起動",
      effects: [
        `${name} とその子プロセスを停止します。`,
        `停止が完了したら、${name} を起動します。`,
      ],
    },
    kill: {
      verb: "強制終了",
      effects: [
        `${name} とその子プロセスを強制終了します。`,
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
