import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import { sessionSchema } from "./protocol";

type AuthenticationState =
  | { kind: "checking" }
  | { kind: "signed-out" }
  | { kind: "signed-in"; csrf: string };

export class AuthenticationRequired extends Error {}

export function AuthenticationGate({
  renderApp,
}: {
  renderApp: (
    signedOut: () => void,
    logout: () => Promise<void>,
  ) => ComponentChildren;
}) {
  const [state, setState] = useState<AuthenticationState>({ kind: "checking" });
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function check() {
    setError("");
    try {
      const response = await fetch("/api/session", { cache: "no-store" });
      if (response.status === 401) {
        setState({ kind: "signed-out" });
        return;
      }
      if (!response.ok) throw new Error();
      const { token } = sessionSchema.parse(await response.json());
      setState({ kind: "signed-in", csrf: token });
    } catch {
      setError("接続できませんでした。サーバーの状態を確認してください。");
    }
  }
  useEffect(() => {
    void check();
  }, []);
  async function login(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      const response = await fetch("/api/login", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Procshelf-Login": "1",
        },
        body: JSON.stringify({ access_key: key.trim() }),
      });
      if (!response.ok) {
        setError(
          response.status === 401
            ? "アクセスキーが正しくありません。"
            : response.status === 429
              ? "ログイン試行が多すぎます。1分ほど待ってから再試行してください。"
              : "ログインできませんでした。",
        );
        return;
      }
      setKey("");
      await check();
    } catch {
      setError("接続できませんでした。もう一度お試しください。");
    } finally {
      setBusy(false);
    }
  }
  async function logout() {
    if (state.kind !== "signed-in" || busy) return;
    setBusy(true);
    try {
      const response = await fetch("/api/logout", {
        method: "POST",
        headers: { "X-Procshelf-Token": state.csrf },
      });
      if (!response.ok && response.status !== 401) throw new Error();
      setError("");
      setState({ kind: "signed-out" });
    } catch {
      setError(
        "ログアウトできませんでした。接続を確認して再試行してください。",
      );
    } finally {
      setBusy(false);
    }
  }
  if (state.kind === "signed-in")
    return (
      <>
        {error && (
          <p class="notice error" role="alert">
            {error}
          </p>
        )}
        {renderApp(() => setState({ kind: "signed-out" }), logout)}
      </>
    );
  return (
    <main>
      <header class="masthead">
        <h1>Procshelf</h1>
      </header>
      <section class="login-panel" aria-labelledby="login-title">
        <h2 id="login-title">ログイン</h2>
        {state.kind === "checking" ? (
          <>
            {!error && <p role="status">接続を確認しています。</p>}
            {error && (
              <>
                <p role="alert">{error}</p>
                <button onClick={() => void check()}>再試行</button>
              </>
            )}
          </>
        ) : (
          <form onSubmit={(event) => void login(event)}>
            <p>プロセスの閲覧・操作にはアクセスキーが必要です。</p>
            <label for="access-key">アクセスキー</label>
            <input
              id="access-key"
              type="password"
              name="password"
              autoComplete="current-password"
              required
              value={key}
              onInput={(event) => setKey(event.currentTarget.value)}
              aria-describedby="key-help"
            />
            <p id="key-help" class="login-help">
              管理サービスが作成したキーを入力してください。
            </p>
            {error && (
              <p class="login-error" role="alert">
                {error}
              </p>
            )}
            <button class="login-submit" disabled={busy}>
              {busy ? "確認中…" : "ログイン"}
            </button>
          </form>
        )}
      </section>
    </main>
  );
}
