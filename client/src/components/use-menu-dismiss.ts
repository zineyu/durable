import { useCallback, useState } from 'react';
import type { FocusEvent, KeyboardEvent } from 'react';

export type MenuDismiss = {
  open: boolean;
  setOpen: (open: boolean) => void;
  toggle: () => void;
  close: () => void;
  onBlur: (event: FocusEvent<HTMLElement>) => void;
  onKeyDown: (event: KeyboardEvent<HTMLElement>) => void;
};

/// 菜单的开合与关闭：焦点移出容器、按 Escape 都关闭，焦点仍在菜单内时保持打开。
export function useMenuDismiss(initiallyOpen = false): MenuDismiss {
  const [open, setOpen] = useState(initiallyOpen);

  const toggle = useCallback(() => {
    setOpen((current) => !current);
  }, []);

  const close = useCallback(() => {
    setOpen(false);
  }, []);

  const onBlur = useCallback((event: FocusEvent<HTMLElement>) => {
    if (event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) {
      return;
    }
    setOpen(false);
  }, []);

  const onKeyDown = useCallback((event: KeyboardEvent<HTMLElement>) => {
    if (event.key === 'Escape') {
      setOpen(false);
    }
  }, []);

  return { open, setOpen, toggle, close, onBlur, onKeyDown };
}
