// Side-effect-free helpers shared by the menu and the backend. Kept apart from
// `utils.ts`, which touches the file system and Electron's `app` on import.

export const ToastStyles = {
  logo: 1,
  banner_centered_top: 2,
  hero: 3,
  banner_top_custom: 4,
  banner_centered_bottom: 5,
  banner_bottom: 6,
  legacy: 7,
};

export const urgencyLevels = [
  { name: 'Low', value: 'low' } as const,
  { name: 'Normal', value: 'normal' } as const,
  { name: 'High', value: 'critical' } as const,
];

export const snakeToCamel = (string_: string) =>
  string_.replaceAll(/([-_][a-z]|^[a-z])/g, (group) =>
    group.toUpperCase().replace('-', ' ').replace('_', ' '),
  );

export const secondsToMinutes = (seconds: number) => {
  const minutes = Math.floor(seconds / 60);
  const secondsLeft = seconds % 60;
  return `${minutes}:${secondsLeft < 10 ? '0' : ''}${secondsLeft}`;
};
