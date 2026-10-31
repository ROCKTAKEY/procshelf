import type { Action } from "./protocol";

export async function confirmAction(
  action: Action,
  name: string,
  ask: (message: string, label: string) => boolean | Promise<boolean>,
): Promise<boolean> {
  if (action === "start") return true;
  const messages = {
    stop: `${name} と配下のプロセスを停止しますか？`,
    restart: `${name} と配下のプロセスを停止して、再起動しますか？`,
    kill: `${name} と配下のプロセスを強制終了しますか？`,
  };
  const labels = {
    stop: "停止する",
    restart: "再起動する",
    kill: "強制終了する",
  };
  return ask(messages[action], labels[action]);
}
