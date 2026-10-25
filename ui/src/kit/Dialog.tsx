import type { ReactNode } from "react";
import { Dialog as AriaDialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { Button } from "./Button";

export interface DialogProps {
  open: boolean;
  onOpenChange(open: boolean): void;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
  /** Prevents closing by clicking outside, for dialogs that guard unsaved work. */
  modal?: boolean;
}

export function Dialog({ open, onOpenChange, title, children, footer, width = 440, modal }: DialogProps) {
  return (
    <ModalOverlay className="overlay" isOpen={open} onOpenChange={onOpenChange} isDismissable={!modal}>
      <Modal className="modal" style={{ width }}>
        <AriaDialog className="dialog">
          <Heading slot="title" className="dialog-title">
            {title}
          </Heading>
          <div className="dialog-body">{children}</div>
          {footer && <div className="dialog-footer">{footer}</div>}
        </AriaDialog>
      </Modal>
    </ModalOverlay>
  );
}

export interface ConfirmProps {
  open: boolean;
  title: string;
  message: ReactNode;
  confirm: string;
  cancel: string;
  /** Optional third choice, e.g. "Discard" next to "Save". */
  alternative?: string;
  danger?: boolean;
  onConfirm(): void;
  onAlternative?(): void;
  onCancel(): void;
}

export function ConfirmDialog(p: ConfirmProps) {
  return (
    <Dialog
      open={p.open}
      onOpenChange={(o) => !o && p.onCancel()}
      title={p.title}
      modal
      footer={
        <>
          <Button variant="ghost" onPress={p.onCancel}>
            {p.cancel}
          </Button>
          {p.alternative && (
            <Button variant="danger" onPress={p.onAlternative}>
              {p.alternative}
            </Button>
          )}
          <Button variant={p.danger ? "danger" : "primary"} onPress={p.onConfirm} autoFocus>
            {p.confirm}
          </Button>
        </>
      }
    >
      <p className="dialog-message">{p.message}</p>
    </Dialog>
  );
}
