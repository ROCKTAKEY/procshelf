import { render } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import {
  canStart,
  logsSchema,
  sessionSchema,
  snapshotSchema,
  stateLabels,
} from "./protocol";
import type { Action, Logs, Process, Snapshot } from "./protocol";
import "./style.css";

async function json(url: string, options?: RequestInit): Promise<unknown> {
  const response = await fetch(url, { ...options, cache: "no-store" });
  if (!response.ok) {
    const body: unknown = await response.json().catch(() => null);
    throw new Error(
      body &&
        typeof body === "object" &&
        "error" in body &&
        typeof body.error === "string"
        ? body.error
        : `HTTP ${response.status}`,
    );
  }
  return response.json();
}
function ActionIcon({ action }: { action: Action }) {
  const paths: Record<Action, string> = {
    start: "M7 4 19 12 7 20Z",
    stop: "M6 6H18V18H6Z",
    restart: "M20 7V3M20 7H16M20 7A9 9 0 1 0 21 14",
    kill: "M5 5 19 19M19 5 5 19",
  };
  return (
    <svg
      class="action-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.8"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d={paths[action]} />
    </svg>
  );
}

function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [connected, setConnected] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [logs, setLogs] = useState<Logs | null>(null);
  const [logError, setLogError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [follow, setFollow] = useState(true);
  const token = useRef("");
  const logPane = useRef<HTMLPreElement>(null);
  const process = snapshot?.processes.find((p) => p.id === selected) ?? null;

  useEffect(() => {
    let disposed = false;
    let events: EventSource | undefined;
    let retry: ReturnType<typeof setTimeout>;
    async function connect() {
      try {
        token.current = sessionSchema.parse(await json("/api/session")).token;
        if (disposed) return;
        events = new EventSource("/api/events");
        events.addEventListener("state", (event) => {
          try {
            const next = snapshotSchema.parse(
              JSON.parse((event as MessageEvent<string>).data),
            );
            if (disposed) return;
            setSnapshot(next);
            setConnected(true);
          } catch {
            setConnected(false);
            setError(
              "サーバーの応答を読み取れません。ページを再読み込みしてください。",
            );
          }
        });
        events.onerror = () => {
          setConnected(false);
          events?.close();
          if (!disposed) retry = setTimeout(connect, 1500);
        };
      } catch {
        if (!disposed) {
          setConnected(false);
          retry = setTimeout(connect, 1500);
        }
      }
    }
    void connect();
    return () => {
      disposed = true;
      events?.close();
      clearTimeout(retry);
    };
  }, []);

  useEffect(() => {
    if (snapshot && !snapshot.processes.some((p) => p.id === selected)) {
      setSelected(snapshot.processes[0]?.id ?? null);
    }
  }, [snapshot, selected]);

  useEffect(() => {
    setLogs(null);
    setLogError(null);
    if (!selected || !connected) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    async function update() {
      try {
        const next = logsSchema.parse(
          await json(`/api/processes/${encodeURIComponent(selected!)}/logs`),
        );
        if (!disposed) {
          setLogs(next);
          setLogError(null);
        }
      } catch {
        if (!disposed)
          setLogError("ログを取得できません。再接続を待っています。");
      }
      if (!disposed) timer = setTimeout(update, 700);
    }
    void update();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [selected, connected]);

  useEffect(() => {
    if (follow && logPane.current)
      logPane.current.scrollTop = logPane.current.scrollHeight;
  }, [logs, follow]);

  async function act(action: Action, target: Process) {
    if (!snapshot || pending || !connected) return;
    if (
      action === "kill" &&
      !confirm(`${target.name} と配下のプロセスを強制終了しますか？`)
    )
      return;
    setPending(true);
    setError(null);
    try {
      await json(`/api/processes/${encodeURIComponent(target.id)}/actions`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Procshelf-Token": token.current,
        },
        body: JSON.stringify({
          action,
          expected_run_id: target.run_id,
          config_generation: snapshot.config_generation,
        }),
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : "操作に失敗しました");
    } finally {
      setPending(false);
    }
  }

  const busy = pending || !connected || snapshot?.shutting_down;
  const active =
    process && ["starting", "running", "stopping"].includes(process.state);
  const filtered =
    snapshot?.processes.filter((p) =>
      `${p.name} ${p.id}`.toLowerCase().includes(filter.toLowerCase()),
    ) ?? [];
  return (
    <main>
      <header class="masthead">
        <h1>Procshelf</h1>
        <div class={`connection ${connected ? "online" : ""}`} role="status">
          <span class="dot" />
          {connected ? "接続中" : "再接続しています"}
        </div>
      </header>
      {error && (
        <div class="notice error" role="alert">
          {error}
          <button
            class="dismiss"
            onClick={() => setError(null)}
            aria-label="エラーを閉じる"
          >
            ×
          </button>
        </div>
      )}
      {snapshot?.config_error && (
        <div class="notice warning" role="alert">
          <strong>設定を読み込めません</strong>
          <br />
          {snapshot.config_error}
          <br />
          <small>最後に読み込めた設定で動作を継続しています。</small>
        </div>
      )}
      {snapshot && snapshot.listen !== snapshot.configured_listen && (
        <div class="notice warning">
          待ち受け先の変更は管理サービスの再起動で適用されます。
        </div>
      )}
      {!connected && snapshot && (
        <div class="notice">
          表示は最後に取得した状態です。接続が戻るまで操作できません。
        </div>
      )}
      {snapshot?.shutting_down && (
        <div class="notice warning">
          管理サービスを終了しています。配下の停止完了を待っています。
        </div>
      )}
      <div class="workspace">
        <aside class="shelf">
          <div class="section-heading">
            <h2>プロセス</h2>
            <span class="count">{snapshot?.processes.length ?? "—"}</span>
          </div>
          <input
            class="search"
            type="search"
            aria-label="プロセスを検索"
            placeholder="名前・IDで検索"
            value={filter}
            onInput={(e) => setFilter(e.currentTarget.value)}
          />
          <nav aria-label="登録されたプロセス">
            {filtered.map((p) => (
              <button
                key={p.id}
                class={`process-row ${selected === p.id ? "selected" : ""}`}
                aria-current={selected === p.id ? "true" : undefined}
                onClick={() => setSelected(p.id)}
              >
                <span class="process-name">{p.name}</span>
                <span class={`status ${p.state}`}>{stateLabels[p.state]}</span>
                {(p.config_pending || p.removed) && (
                  <span class="card-note">
                    {p.removed ? "設定から削除済み" : "設定変更あり"}
                  </span>
                )}
              </button>
            ))}
          </nav>
          {snapshot && !filtered.length && (
            <p class="empty-small">
              {snapshot.processes.length
                ? "一致する登録がありません。"
                : "config.toml にプロセスを登録すると、ここに表示されます。"}
            </p>
          )}
        </aside>
        <section class="detail" aria-label="プロセスの詳細">
          {process ? (
            <>
              <div class="detail-heading">
                <h2>{process.name}</h2>
                <span class={`status ${process.state}`}>
                  {stateLabels[process.state]}
                </span>
              </div>
              <div class="actions">
                {active ? (
                  <button
                    class="primary"
                    disabled={busy || process.state === "stopping"}
                    onClick={() => void act("stop", process)}
                  >
                    <ActionIcon action="stop" />
                    <span>
                      {process.state === "stopping" ? "停止処理中" : "停止"}
                    </span>
                  </button>
                ) : (
                  <button
                    class="primary"
                    disabled={busy || !canStart(process.state, process.removed)}
                    onClick={() => void act("start", process)}
                  >
                    <ActionIcon action="start" />
                    <span>起動</span>
                  </button>
                )}
                {active && (
                  <>
                    <button
                      disabled={
                        busy || process.state === "stopping" || process.removed
                      }
                      onClick={() => void act("restart", process)}
                    >
                      <ActionIcon action="restart" />
                      <span>再起動</span>
                    </button>
                    <span class="action-separator" aria-hidden="true" />
                    <button
                      class="danger"
                      disabled={busy}
                      onClick={() => void act("kill", process)}
                    >
                      <ActionIcon action="kill" />
                      <span>強制終了</span>
                    </button>
                  </>
                )}
              </div>
              {process.config_pending && (
                <p class="inline-note">
                  設定変更あり。表示は次回起動の設定です。再起動で適用します。
                </p>
              )}
              {process.removed && (
                <p class="inline-note">
                  設定から削除済み。停止完了後に一覧から消えます。
                </p>
              )}
              {process.outcome && (
                <p class="outcome">終了・診断: {process.outcome}</p>
              )}
              <details class="configuration" key={process.id}>
                <summary>実行情報・設定</summary>
                <dl>
                  <dt>ID</dt>
                  <dd>
                    <code>{process.id}</code>
                  </dd>
                  <dt>PID</dt>
                  <dd>{process.pid ?? "—"}</dd>
                  <dt>実行番号</dt>
                  <dd>{process.run_id ?? "—"}</dd>
                  <dt>自動起動</dt>
                  <dd>{process.autostart ? "有効" : "無効"}</dd>
                  <dt>コマンド</dt>
                  <dd>
                    <code>{[process.program, ...process.args].join(" ")}</code>
                  </dd>
                  <dt>作業ディレクトリ</dt>
                  <dd>
                    <code>{process.cwd}</code>
                  </dd>
                </dl>
              </details>
              <div class="log-heading">
                <h3>出力ログ</h3>
                <label>
                  <input
                    type="checkbox"
                    checked={follow}
                    onChange={(e) => setFollow(e.currentTarget.checked)}
                  />
                  末尾を追尾
                </label>
              </div>
              {logError && (
                <p class="inline-note" role="status">
                  {logError}
                </p>
              )}
              {!!logs?.discarded && (
                <p class="inline-note">
                  保持上限により、古いログ {logs.discarded} 件を省略しています。
                </p>
              )}
              <pre
                class="logs"
                ref={logPane}
                aria-label="プロセスの出力ログ"
                tabIndex={0}
              >
                {logs?.entries.length ? (
                  logs.entries.map((e) => (
                    <span key={e.seq} class={`log-entry ${e.stream}`}>
                      <span class="log-meta">
                        {new Date(e.time_ms).toLocaleTimeString()} · #{e.run_id}{" "}
                        · {e.stream}
                      </span>
                      <span class="log-text">{e.text}</span>
                    </span>
                  ))
                ) : (
                  <span class="empty-log">出力を待っています。</span>
                )}
              </pre>
              <p class="footnote">
                ログはこの管理サービスの稼働中、上限付きで保持されます。
              </p>
            </>
          ) : (
            <div class="welcome">
              <h2>
                {snapshot
                  ? "登録されたプロセスはありません"
                  : "管理サービスに接続しています"}
              </h2>
              <p>
                {snapshot
                  ? "config.toml にコマンドを登録してください。自動起動するものと、必要なときに起動するものを選べます。"
                  : "接続が完了すると、現在の状態を表示します。"}
              </p>
            </div>
          )}
        </section>
      </div>
    </main>
  );
}
render(<App />, document.getElementById("app")!);
