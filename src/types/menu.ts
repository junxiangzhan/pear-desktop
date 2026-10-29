// Plugin menus: templates that plugins build in the page (`menu(ctx)`) and
// the Rust shell draws as native menus (src/inject/menu-runtime.ts).
// The shape follows Electron's MenuItemConstructorOptions, which the menus
// were written against; these are the fields that reach the native menu.

/** The item a click handler gets; writing `checked` updates the native item */
export interface MenuItem {
  checked: boolean;
}

/** The main window as plugin menus see it */
export interface PluginWindow {
  setAlwaysOnTop(flag: boolean): void;
  webContents: {
    /** Delivers a message to page listeners (`window.ipcRenderer.on`) */
    send(channel: string, ...args: unknown[]): void;
  };
}

export interface MenuItemOptions {
  label?: string;
  /** Not drawn by the native menu (Electron only showed it on macOS) */
  toolTip?: string;
  type?: 'normal' | 'separator' | 'submenu' | 'checkbox' | 'radio';
  checked?: boolean;
  enabled?: boolean;
  visible?: boolean;
  accelerator?: string;
  role?: string;
  submenu?: MenuItemOptions[];
  click?: (item: MenuItem, window: PluginWindow, event: unknown) => unknown;
}

export type MenuTemplate = MenuItemOptions[];
