import type { ButtonHTMLAttributes } from 'react';
import { Icon } from './Icon';
import styles from './IconButton.module.css';
import type { IconName } from './Icon';
import type { IconButtonSize, IconButtonTone } from '../view/types';

export type IconButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  icon: IconName;
  label: string;
  tone?: IconButtonTone;
  size?: IconButtonSize;
};

export function IconButton({
  icon,
  label,
  tone = 'neutral',
  size = 'sm',
  className,
  ...rest
}: IconButtonProps) {
  const classes = [styles.button, styles[tone], styles[size], className].filter(Boolean).join(' ');
  return (
    <button type="button" className={classes} aria-label={label} title={label} {...rest}>
      <Icon name={icon} />
    </button>
  );
}
