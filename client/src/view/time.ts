/// 侧栏显示的 session 更新时间：本地时区的 `MM-DD HH:mm`。
export function formatUpdatedAt(milliseconds: number): string {
  const date = new Date(milliseconds);
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
