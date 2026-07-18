import * as Dialog from "@radix-ui/react-dialog";
import { KeyRound, X } from "lucide-react";
import { useEffect, useState } from "react";

import { tr } from "../i18n";
import { useKomaStore } from "../store/koma";

export function PasswordDialog() {
  const request = useKomaStore((state) => state.passwordRequest);
  const resolve = useKomaStore((state) => state.resolvePassword);
  const [password, setPassword] = useState("");

  useEffect(() => {
    if (request !== null) setPassword("");
  }, [request]);

  return (
    <Dialog.Root
      open={request !== null}
      onOpenChange={(open) => {
        if (!open) resolve(null);
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="password-dialog">
          <div className="password-dialog-mark" aria-hidden="true">
            <KeyRound size={20} />
          </div>
          <Dialog.Title>
            {tr("Unlock “{{title}}”", {
              title: request?.title ?? tr("publication"),
            })}
          </Dialog.Title>
          <Dialog.Description>
            {tr("Password is kept in memory for this session.")}
          </Dialog.Description>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              if (password.length > 0) resolve(password);
            }}
          >
            <label>
              {tr("Password")}
              <input
                autoFocus
                type="password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                autoComplete="off"
              />
            </label>
            <div className="password-dialog-actions">
              <button
                type="button"
                className="secondary-button"
                onClick={() => resolve(null)}
              >
                {tr("Cancel")}
              </button>
              <button
                type="submit"
                className="primary-button"
                disabled={password.length === 0}
              >
                {tr("Unlock")}
              </button>
            </div>
          </form>
          <Dialog.Close asChild>
            <button
              type="button"
              className="dialog-close"
              aria-label={tr("Cancel")}
            >
              <X size={17} />
            </button>
          </Dialog.Close>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
