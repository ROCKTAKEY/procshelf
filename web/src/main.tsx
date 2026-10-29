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
        <div class="brand">
          <span class="brand-mark" aria-hidden="true">
            ▤
          </span>
          <div>
            <h1>Procshelf</h1>
            <p>プロセスを、手元に。</p>
          </div>
        </div>
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
                class={`process-card ${selected === p.id ? "selected" : ""}`}
                aria-current={selected === p.id ? "true" : undefined}
                onClick={() => setSelected(p.id)}
              >
                <span class="process-title">
                  {p.name}
                  <span class={`state-dot ${p.state}`} />
                </span>
                <span class="process-sub">
                  <code>{p.id}</code>
                  <span>{stateLabels[p.state]}</span>
                </span>
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
          <p class="shelf-note">
            登録は <code>config.toml</code> から。
            <br />
            保存した変更は自動で反映されます。
          </p>
        </aside>
        <section class="detail" aria-label="プロセスの詳細">
          {process ? (
            <>
              <div class="detail-heading">
                <div>
                  <span class="eyebrow">PROCESS / {process.id}</span>
                  <h2>{process.name}</h2>
                </div>
                <span class={`status ${process.state}`}>
                  {stateLabels[process.state]}
                </span>
              </div>
              <div class="facts">
                <span>
                  自動起動{" "}
                  <strong>{process.autostart ? "有効" : "無効"}</strong>
                </span>
                <span>
                  PID <strong>{process.pid ?? "—"}</strong>
                </span>
                <span>
                  実行 <strong>{process.run_id ?? "—"}</strong>
                </span>
              </div>
              <div class="command">
                <code>{[process.program, ...process.args].join(" ")}</code>
                <small>作業ディレクトリ: {process.cwd}</small>
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
              <div class="actions">
                <button
                  class="primary"
                  disabled={busy || !canStart(process.state, process.removed)}
                  onClick={() => void act("start", process)}
                >
                  起動
                </button>
                <button
                  disabled={busy || !active || process.state === "stopping"}
                  onClick={() => void act("stop", process)}
                >
                  停止
                </button>
                <button
                  disabled={
                    busy ||
                    !active ||
                    process.state === "stopping" ||
                    process.removed
                  }
                  onClick={() => void act("restart", process)}
                >
                  再起動
                </button>
                <button
                  class="danger"
                  disabled={busy || !active}
                  onClick={() => void act("kill", process)}
                >
                  強制終了
                </button>
              </div>
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
                      {e.text}
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
              <span class="welcome-mark" aria-hidden="true">
                ▤
              </span>
              <h2>
                {snapshot
                  ? "棚にプロセスを追加しましょう"
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
      <footer>
        <span>ブラウザを閉じてもプロセスは動作を継続します。</span>
        <code>{snapshot?.listen ?? "localhost"}</code>
      </footer>
    </main>
  );
}
render(<App />, document.getElementById("app")!);
