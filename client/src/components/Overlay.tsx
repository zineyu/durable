import type { ReactElement, ReactNode } from 'react';

export type OverlayProps = {
  label: string;
  onClose: () => void;
  overlayClassName?: string | undefined;
  panelClassName?: string | undefined;
  children: ReactNode;
};

export function Overlay({
  label,
  onClose,
  overlayClassName,
  panelClassName,
  children,
}: OverlayProps): ReactElement {
  return (
    <div className={overlayClassName} onClick={onClose}>
      <div
        className={panelClassName}
        role="dialog"
        aria-modal="true"
        aria-label={label}
        onClick={(event) => {
          event.stopPropagation();
        }}
      >
        {children}
      </div>
    </div>
  );
}
