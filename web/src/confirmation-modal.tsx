import { useLayoutEffect, useRef, useState } from "preact/hooks";

type Request = {
  message: string;
  label: string;
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
      <h2 id="confirmation-title">{request.label}前に確認</h2>
      <p id="confirmation-message">{request.message}</p>
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
  function ask(message: string, label: string): Promise<boolean> {
    return new Promise((resolve) => setRequest({ message, label, resolve }));
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
