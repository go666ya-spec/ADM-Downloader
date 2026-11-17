/**
 * 图标资源表：统一 24×24 线性风格，描边跟随 currentColor。
 */
export type IconName =
  | 'dashboard'
  | 'download'
  | 'upload'
  | 'sliders'
  | 'settings'
  | 'pause'
  | 'play'
  | 'plus'
  | 'trash'
  | 'folder'
  | 'search'
  | 'close'
  | 'cpu'
  | 'clock'
  | 'rabbit'
  | 'turtle'
  | 'squirrel'
  | 'chevron-right'
  | 'chevron-down'
  | 'file'
  | 'check'
  | 'sparkles'
  | 'globe'
  | 'plugin'
  | 'tune'
  | 'bell'

export const ICON_PATHS: Record<IconName, string> = {
  dashboard: 'M4 13h6V4H4v9Zm0 7h6v-5H4v5Zm10 0h6v-9h-6v9Zm0-16v5h6V4h-6Z',
  download: 'M12 4v10m0 0 4-4m-4 4-4-4M5 18h14',
  upload: 'M12 20V10m0 0 4 4m-4-4-4 4M5 6h14',
  sliders: 'M4 7h10M18 7h2M4 17h4M12 17h8M16 5v4M8 15v4',
  settings:
    'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM20.4 12a8.4 8.4 0 0 0-.1-1.2l1.9-1.5-1.9-3.3-2.3 1a8 8 0 0 0-2-1.2L15.6 2.2h-3.9l-.4 2.6a8 8 0 0 0-2 1.2l-2.3-1-1.9 3.3 1.9 1.5a8.4 8.4 0 0 0 0 2.4l-1.9 1.5 1.9 3.3 2.3-1a8 8 0 0 0 2 1.2l.4 2.6h3.9l.4-2.6a8 8 0 0 0 2-1.2l2.3 1 1.9-3.3-1.9-1.5c.1-.4.1-.8.1-1.2Z',
  pause: 'M9 5v14M15 5v14',
  play: 'M7 4l13 8-13 8V4Z',
  plus: 'M12 5v14M5 12h14',
  trash: 'M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13',
  folder: 'M3 7h6l2 2h10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z',
  search: 'M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14Zm5-2 5 5',
  close: 'M4 4l16 16M20 4 4 20',
  cpu: 'M7 7h10v10H7zM10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4',
  clock: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18ZM12 7v5l3 2',
  rabbit: 'M9 9c-1-2-1-5 1-5s2 3 2 5M15 9c1-2 1-5-1-5s-2 3-2 5M6 14a6 5 0 0 0 12 0v-2H6Z',
  turtle: 'M3 15a9 6 0 0 1 18 0M3 15h18M7 15v3M17 15v3M21 12l2-2',
  squirrel: 'M15 4c3 1 4 4 3 7-1 2.5-4 3.5-4 7M8 8c-3 1.5-3 6 0 8 2 1.5 5 .5 5-1.5',
  'chevron-right': 'M9 5l7 7-7 7',
  'chevron-down': 'M5 9l7 7 7-7',
  file: 'M6 3h7l5 5v13H6zM13 3v5h5',
  check: 'M4 12l5 5L20 6',
  sparkles: 'M12 3l1.8 4.7L18.5 9.5l-4.7 1.8L12 16l-1.8-4.7L5.5 9.5l4.7-1.8L12 3ZM18 15l.9 2.1 2.1.9-2.1.9L18 21l-.9-2.1-2.1-.9 2.1-.9L18 15Z',
  globe: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18ZM3 12h18M12 3c2.5 2.4 3.8 5.4 3.8 9S14.5 18.6 12 21c-2.5-2.4-3.8-5.4-3.8-9S9.5 5.4 12 3Z',
  plugin: 'M9 3v4M15 3v4M6 7h12v5a6 6 0 0 1-12 0V7ZM12 18v3',
  tune: 'M4 6h8M16 6h4M4 12h4M12 12h8M4 18h10M18 18h2M12 4v4M8 10v4M14 16v4',
  bell: 'M12 21a2 2 0 0 0 2-2H10a2 2 0 0 0 2 2ZM6 17h12l-1.5-3V9a4.5 4.5 0 0 0-9 0v5L6 17Z',
}
