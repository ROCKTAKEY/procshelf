import type { Action } from "./protocol";

export function confirmAction(
  action: Action,
  name: string,
  ask: (message: string) => boolean,
): boolean {
  if (action === "start") return true;
  const messages = {
    stop: `${name} と配下のプロセスを停止しますか？`,
    restart: `${name} と配下のプロセスを停止して、再起動しますか？`,
    kill: `${name} と配下のプロセスを強制終了しますか？`,
  };
  return ask(messages[action]);
}
