// The account menu's items that were YTM's own dialogs: its settings, the
// account switcher, sign out, help and feedback, uploading songs. Each does
// what YTM's handler for it does (music_polymer, checked 2026-11-14).
import { config } from './boot';
import { authorization, innertube } from './innertube';

import { parseAccountSwitcher, parseSettings } from '../model/parse';

import type { AccountItem, Endpoint, SettingCategory } from '../model/types';

/**
 * Opens a page outside the app (the system's browser): a new window, which
 * the Android shell hands to the browser (GeckoPage), as Pear's window does
 * with links on the desktop
 */
export const openUrl = (url: string) => {
  window.open(url, '_blank', 'noopener');
};

// ---- PREF: the settings YTM keeps in a cookie ----------------------------
//
// Language (hl), region (gl) and restricted mode (flag 58 of the f flags:
// f2, 1 << 27) live in YouTube's PREF cookie; YTM saves it and reloads.

const RESTRICTED_FLAG = { key: 'f2', bit: 1 << 27 };

const readPref = () =>
  new URLSearchParams(
    document.cookie
      .split('; ')
      .find((pair) => pair.startsWith('PREF='))
      ?.slice(5) ?? '',
  );

const savePref = (pref: URLSearchParams) => {
  const twoYears = 2 * 365 * 24 * 60 * 60;
  document.cookie = `PREF=${pref.toString()}; domain=.youtube.com; path=/; max-age=${twoYears}; secure`;
};

const restricted = () =>
  ((parseInt(readPref().get(RESTRICTED_FLAG.key) ?? '0', 16) || 0) &
    RESTRICTED_FLAG.bit) !==
  0;

interface ClientSetting {
  clientSettingEnum?: { item?: string };
  stringValue?: string;
  boolValue?: boolean;
}

const saveClientSettings = (settings: ClientSetting[]) => {
  const pref = readPref();
  for (const setting of settings) {
    switch (setting.clientSettingEnum?.item) {
      case 'I18N_LANGUAGE':
        pref.set('hl', setting.stringValue ?? '');
        break;
      case 'I18N_REGION':
        pref.set('gl', setting.stringValue ?? '');
        break;
      case 'SAFETY_MODE': {
        const flags = parseInt(pref.get(RESTRICTED_FLAG.key) ?? '0', 16) || 0;
        const next = setting.boolValue
          ? flags | RESTRICTED_FLAG.bit
          : flags & ~RESTRICTED_FLAG.bit;
        if (next) pref.set(RESTRICTED_FLAG.key, next.toString(16));
        else pref.delete(RESTRICTED_FLAG.key);
        break;
      }
      default:
        throw new Error('unsupported client setting');
    }
  }
  savePref(pref);
  // The page reads them as it loads (YTM reloads too)
  location.reload();
};

// ---- Settings ----------------------------------------------------------

/** YTM's settings; the ones it keeps in the cookie are read from it */
export const readSettings = async (): Promise<SettingCategory[]> => {
  const categories = parseSettings(await innertube('/account/get_setting', {}));
  const current: Record<string, string | undefined> = {
    I18N_LANGUAGE: config<string>('HL'),
    I18N_REGION: config<string>('GL'),
  };
  for (const category of categories) {
    for (const item of category.items) {
      if (item.kind === 'toggle' && item.id === 'SAFETY_MODE') {
        item.on = restricted();
      }
      if (item.kind === 'choice' && item.id && item.id in current) {
        const value = current[item.id];
        for (const option of item.options) {
          option.selected = option.value === value;
        }
      }
    }
  }
  return categories;
};

/** YTM's toast for an endpoint ("Changes saved"), if it has one */
const toastOf = (endpoint: Endpoint) => {
  let found: string | undefined;
  JSON.stringify(endpoint, (key, value: unknown) => {
    if (!found && key === 'responseText') {
      const node = value as {
        simpleText?: string;
        runs?: { text?: string }[];
      };
      found =
        node.simpleText ?? node.runs?.map((run) => run.text ?? '').join('');
    }
    return value;
  });
  return found || undefined;
};

/**
 * Runs a setting's endpoint, as YTM does: a server setting, a feedback
 * token (the history's), the cookie's settings (reloads the page), or a
 * link. Resolves to what YTM says about it.
 */
export const runSetting = async (
  endpoint: Endpoint,
): Promise<string | undefined> => {
  const server = endpoint.setSettingEndpoint as
    | { settingItemId?: string }
    | undefined;
  if (server) {
    await innertube('/account/set_setting', {
      newValue: server,
      settingItemId: server.settingItemId,
    });
    return toastOf(endpoint);
  }
  const feedback = endpoint.feedbackEndpoint as
    | { feedbackToken?: string }
    | undefined;
  if (feedback?.feedbackToken) {
    await innertube('/feedback', { feedbackTokens: [feedback.feedbackToken] });
    return toastOf(endpoint);
  }
  const client = endpoint.setClientSettingEndpoint as
    | { settingDatas?: ClientSetting[] }
    | undefined;
  if (client?.settingDatas) {
    saveClientSettings(client.settingDatas);
    return undefined;
  }
  const url = (endpoint.urlEndpoint as { url?: string } | undefined)?.url;
  if (url) {
    openUrl(url);
    return undefined;
  }
  throw new Error('unsupported setting endpoint');
};

// ---- Accounts ----------------------------------------------------------

/** The accounts and channels to switch to (YTM's servlet, a GET) */
export const readAccounts = async () => {
  const response = await fetch('/getAccountSwitcherEndpoint', {
    credentials: 'include',
  });
  if (!response.ok) throw new Error(`accounts: HTTP ${response.status}`);
  const body = await response.text();
  // A JSON-hijacking guard comes first
  return parseAccountSwitcher(JSON.parse(body.slice(body.indexOf('{'))));
};

/** Switches account or channel: YouTube's sign-in handler, then back here */
export const switchAccount = (signinUrl: string) => location.assign(signinUrl);

/** Signs out (YTM's LOGOUT_URL) */
export const signOut = () =>
  location.assign(config<string>('LOGOUT_URL') || '/logout');

/** Google's page for signing in to one more account, in the app */
export const addAccount = (url: string) => location.assign(url);

const HELP_URL = 'https://support.google.com/youtubemusic';
// YTM's feedback form is Google's in-page widget, gone with its interface;
// YouTube Music's help community takes feedback instead
const FEEDBACK_URL = 'https://support.google.com/youtubemusic/community';

/** What an account menu item does, by YTM's endpoint for it */
export type AccountAction =
  | { kind: 'page' }
  | { kind: 'settings' }
  | { kind: 'accounts' }
  | { kind: 'signOut' }
  | { kind: 'upload' }
  | { kind: 'url'; url: string };

export const accountAction = (item: AccountItem): AccountAction | undefined => {
  if (item.open) return { kind: 'page' };
  const link = item.ytm.compactLinkRenderer as
    | { navigationEndpoint?: Endpoint; serviceEndpoint?: Endpoint }
    | undefined;
  const endpoint = { ...link?.serviceEndpoint, ...link?.navigationEndpoint };
  if (endpoint.applicationSettingsEndpoint) return { kind: 'settings' };
  if (endpoint.getAccountSwitcherEndpoint) return { kind: 'accounts' };
  if (endpoint.signOutEndpoint) return { kind: 'signOut' };
  const url = (endpoint.urlEndpoint as { url?: string } | undefined)?.url;
  if (url) return { kind: 'url', url };
  // Client signals: the signal names YTM's dialog
  const actions = (
    endpoint.signalServiceEndpoint as
      | {
          actions?: {
            signalAction?: { signal?: string };
            sendFeedbackAction?: unknown;
          }[];
        }
      | undefined
  )?.actions;
  for (const action of actions ?? []) {
    const signal = action.signalAction?.signal;
    if (signal === 'OPEN_FILE_PICKER') return { kind: 'upload' };
    if (signal === 'HELP') return { kind: 'url', url: HELP_URL };
    if (action.sendFeedbackAction) return { kind: 'url', url: FEEDBACK_URL };
  }
  return undefined;
};

// ---- Uploads -----------------------------------------------------------
//
// YTM uploads to Google's resumable upload service: a start request names
// the file and its size and gets the upload's address, then the file goes
// there in one request.

const UPLOAD_URL = 'https://upload.youtube.com/upload/usermusic/http';
/** What YTM takes */
export const UPLOAD_TYPES = '.flac,.m4a,.mp3,.ogg,.wma,audio/*';

export const uploadSong = async (file: File) => {
  const authUser = String(config('SESSION_INDEX') ?? 0);
  const headers: Record<string, string> = {
    'Content-Type': 'application/x-www-form-urlencoded;charset=utf-8',
    'X-Goog-Upload-Command': 'start',
    'X-Goog-Upload-Header-Content-Length': String(file.size),
    'X-Goog-Upload-Protocol': 'resumable',
    'X-Goog-AuthUser': authUser,
  };
  const auth = await authorization();
  if (auth) headers.Authorization = auth;
  const start = await fetch(`${UPLOAD_URL}?authuser=${authUser}`, {
    method: 'POST',
    credentials: 'include',
    headers,
    body: `filename=${encodeURIComponent(file.name)}`,
  });
  const target = start.headers.get('X-Goog-Upload-URL');
  if (!start.ok || !target) {
    throw new Error(`upload start: HTTP ${start.status}`);
  }
  const sent = await fetch(target, {
    method: 'POST',
    credentials: 'include',
    headers: {
      'X-Goog-Upload-Command': 'upload, finalize',
      'X-Goog-Upload-Offset': '0',
    },
    body: file,
  });
  if (!sent.ok) throw new Error(`upload: HTTP ${sent.status}`);
};
