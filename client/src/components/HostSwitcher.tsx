import type { ReactElement } from 'react';
import type { HostName } from '../api/generated/HostName';
import { IconButton } from './IconButton';
import styles from './HostSwitcher.module.css';
import { StatusDot } from './StatusDot';
import { useMenuDismiss } from './use-menu-dismiss';
import type { SessionId } from '../view/ids';
import type { HostView, SessionSummary, StatusTone } from '../view/types';

export type HostSwitcherProps = {
  hosts: HostView[];
  sessions: SessionSummary[];
  selectedHost: HostName | null;
  selectedSessionId?: SessionId | null;
  onSelectSession?: (sessionId: SessionId) => void;
  onRegister?: () => void;
  onRename?: (host: HostName) => void;
  onRemove?: (host: HostName) => void;
  onToggleEnabled?: (host: HostView) => void;
  onReconnect?: (host: HostName) => void;
  defaultMenuOpen?: boolean;
};

/// host 面板：下拉里按 host 分组列出 session，并给出注册、改名、删除、停用与重连。
/// session 的选中位置由 `App.tsx` 通过 URL 持有，本组件只发出命令。
/// 规格 user story R-46：注册、改名、删除、停用与手动重连都在这个面板里。
export function HostSwitcher({
  hosts,
  sessions,
  selectedHost,
  selectedSessionId = null,
  onSelectSession,
  onRegister,
  onRename,
  onRemove,
  onToggleEnabled,
  onReconnect,
  defaultMenuOpen = false,
}: HostSwitcherProps): ReactElement {
  const {
    open: menuOpen,
    toggle,
    close,
    onBlur,
    onKeyDown,
  } = useMenuDismiss(defaultMenuOpen);

  return (
    <nav
      className={styles.switcher}
      aria-label="host 切换器"
      onBlur={onBlur}
      onKeyDown={onKeyDown}
    >
      <div className={styles.bar}>
        <button
          type="button"
          className={styles.trigger}
          aria-haspopup="true"
          aria-expanded={menuOpen}
          onClick={toggle}
        >
          <StatusDot tone={selectedHost === null ? 'idle' : toneOf(hosts, selectedHost)} />
          <span className={styles.name}>{selectedHost ?? 'host'}</span>
        </button>
        <IconButton icon="plus" label="注册 host" onClick={() => onRegister?.()} />
      </div>
      {menuOpen ? (
        <div className={styles.panel}>
          {hosts.length === 0 ? <p className={styles.empty}>还没有 host</p> : null}
          {hosts.map((host) => {
            const hostSessions = sessions.filter((session) => session.host === host.name);
            return (
              <section key={host.name} className={styles.host} aria-label={`host ${host.name}`}>
                <div className={styles.hostHeader}>
                  <StatusDot tone={host.tone} />
                  <span className={styles.name}>{host.name}</span>
                  <span className={styles.status}>{host.label}</span>
                  {host.latencyMs === null ? null : (
                    <span className={styles.latency}>往返 {host.latencyMs} ms</span>
                  )}
                </div>
                <p className={styles.target}>
                  {host.kind === 'ssh' ? host.target : '本机'}
                </p>
                {host.notice === null ? null : (
                  <p className={styles.detail}>{host.notice}</p>
                )}
                <div className={styles.hostActions}>
                  <button
                    type="button"
                    className={styles.action}
                    onClick={() => { onToggleEnabled?.(host); }}
                  >
                    {host.enabled ? '停用' : '启用'}
                  </button>
                  <button
                    type="button"
                    className={styles.action}
                    onClick={() => { onReconnect?.(host.name); }}
                  >
                    重连
                  </button>
                  <IconButton
                    icon="pencil"
                    label={`重命名 host：${host.name}`}
                    size="sm"
                    disabled={!host.available}
                    onClick={() => { close(); onRename?.(host.name); }}
                  />
                  <IconButton
                    icon="trash"
                    label={`删除 host：${host.name}`}
                    tone="danger"
                    size="sm"
                    disabled={!host.available}
                    onClick={() => { close(); onRemove?.(host.name); }}
                  />
                </div>
                {host.available ? (
                  <ul className={styles.sessions}>
                    {hostSessions.length === 0 ? (
                      <li className={styles.empty}>这个 host 没有 session</li>
                    ) : null}
                    {hostSessions.map((session) => (
                      <li key={session.id}>
                        <button
                          type="button"
                          className={styles.session}
                          aria-current={session.id === selectedSessionId}
                          onClick={() => {
                            close();
                            onSelectSession?.(session.id);
                          }}
                        >
                          {session.title}
                        </button>
                      </li>
                    ))}
                  </ul>
                ) : null}
              </section>
            );
          })}
        </div>
      ) : null}
    </nav>
  );
}

function toneOf(hosts: HostView[], name: HostName): StatusTone {
  const host = hosts.find((entry) => entry.name === name);
  return host === undefined ? 'idle' : host.tone;
}
