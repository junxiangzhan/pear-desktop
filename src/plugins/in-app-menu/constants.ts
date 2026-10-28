export interface InAppMenuConfig {
  enabled: boolean;
  hideDOMWindowControls: boolean;
}
// Off by default on macOS and Linux, which keep the native title bar.
// Android reports Linux too; the app turns in-app-menu on there anyway.
const userAgent = navigator.userAgent.toLowerCase();

export const defaultInAppMenuConfig: InAppMenuConfig = {
  enabled: !userAgent.includes('mac') && !userAgent.includes('linux'),
  hideDOMWindowControls: false,
};
