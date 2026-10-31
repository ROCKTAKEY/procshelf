import { useLayoutEffect, useRef, useState } from "preact/hooks";

import type { Confirmation } from "./confirm-action";

type Request = Confirmation & {
  resolve: (approved: boolean) => void;
};

function ConfirmationModal({
  request,
  finish,
}: {
  request: Request;
  finish: (approved: boolean) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useLayoutEffect(() => {
    const element = dialog.current!;
    element.showModal();
    return () => element.close();
  }, []);
  return (
    <dialog
      ref={dialog}
      class="confirmation-modal"
      aria-labelledby="confirmation-title"
      aria-describedby="confirmation-message"
      onCancel={(event) => {
        event.preventDefault();
        finish(false);
      }}
    >
      <h2 id="confirmation-title">{request.title}</h2>
      {request.sequence ? (
        <ol id="confirmation-message" class="confirmation-flow">
          {request.effects.map((effect, index) => (
            <li key={effect}>
              <span class="flow-step">{index + 1}</span>
              <span>{effect}</span>
            </li>
          ))}
        </ol>
      ) : (
        <ul id="confirmation-message" class="confirmation-effects">
          {request.effects.map((effect) => (
            <li key={effect}>{effect}</li>
          ))}
        </ul>
      )}
      <div class="confirmation-actions">
        <button autoFocus onClick={() => finish(false)}>
          キャンセル
        </button>
        <button class="confirm-execution" onClick={() => finish(true)}>
          {request.label}
        </button>
      </div>
    </dialog>
  );
}

export function useConfirmationModal() {
  const [request, setRequest] = useState<Request | null>(null);
  function ask(confirmation: Confirmation): Promise<boolean> {
    return new Promise((resolve) => setRequest({ ...confirmation, resolve }));
  }
  function finish(approved: boolean) {
    request?.resolve(approved);
    setRequest(null);
  }
  return {
    ask,
    modal: request && <ConfirmationModal request={request} finish={finish} />,
  };
}
