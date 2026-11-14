// YouTube Music's API (innertube), called directly: what YTM's network
// manager did, from the page's ytcfg (client context, visitor id, session).
// Same origin, so the account's cookies go along; signed in, requests also
// carry the SAPISIDHASH authorization YouTube's own clients compute.
import { createSignal } from 'solid-js';

import { config } from './boot';

const cookie = (name: string) =>
  document.cookie
    .split('; ')
    .find((pair) => pair.startsWith(`${name}=`))
    ?.slice(name.length + 1);

const sha1 = async (text: string) => {
  const digest = await crypto.subtle.digest(
    'SHA-1',
    new TextEncoder().encode(text),
  );
  return Array.from(new Uint8Array(digest), (byte) =>
    byte.toString(16).padStart(2, '0'),
  ).join('');
};

/** `SAPISIDHASH <time>_<sha1(time sapisid origin)>`, and its 1P/3P forms */
export const authorization = async () => {
  const time = Math.floor(Date.now() / 1000);
  const origin = location.origin;
  const parts: string[] = [];
  for (const [scheme, name] of [
    ['SAPISIDHASH', 'SAPISID'],
    ['SAPISID1PHASH', '__Secure-1PAPISID'],
    ['SAPISID3PHASH', '__Secure-3PAPISID'],
  ] as const) {
    const value = cookie(name);
    if (value) {
      parts.push(`${scheme} ${time}_${await sha1(`${time} ${value} ${origin}`)}`);
    }
  }
  return parts.length ? parts.join(' ') : undefined;
};

// Read from ytcfg once the page is up; assumed signed in until then, so a
// signed-in user does not see a sign-in button flash by
const [signedIn, setSignedIn] = createSignal(true);
export { signedIn };

/** Reads the sign-in state; it does not change without a reload */
export const readSignedIn = () => {
  const yes = config('LOGGED_IN') === true;
  setSignedIn(yes);
  return yes;
};

/** YTM's sign-in link: Google's sign-in, back to YouTube Music */
export const signIn = () => {
  const hl = config<string>('HL') ?? 'en';
  const next = new URL('https://www.youtube.com/signin');
  next.searchParams.set('action_handle_signin', 'true');
  next.searchParams.set('app', 'desktop');
  next.searchParams.set('hl', hl);
  next.searchParams.set('next', `${location.origin}/`);
  const url = new URL('https://accounts.google.com/ServiceLogin');
  url.searchParams.set('ltmpl', 'music');
  url.searchParams.set('service', 'youtube');
  url.searchParams.set('uilel', '3');
  url.searchParams.set('passive', 'true');
  url.searchParams.set('continue', next.toString());
  url.searchParams.set('hl', hl);
  location.assign(url.toString());
};

/** An innertube request: `path` is the API's, like `/browse` or `/next` */
export const innertube = async <T = unknown>(
  path: string,
  body: Record<string, unknown>,
): Promise<T> => {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    'X-Origin': location.origin,
  };
  const visitor = config<string>('VISITOR_DATA');
  if (visitor) headers['X-Goog-Visitor-Id'] = visitor;
  const clientName = config<number | string>('INNERTUBE_CONTEXT_CLIENT_NAME');
  if (clientName !== undefined) {
    headers['X-Youtube-Client-Name'] = String(clientName);
  }
  const clientVersion = config<string>('INNERTUBE_CLIENT_VERSION');
  if (clientVersion) headers['X-Youtube-Client-Version'] = clientVersion;
  if (signedIn()) {
    const auth = await authorization();
    if (auth) headers.Authorization = auth;
    headers['X-Goog-AuthUser'] = String(config('SESSION_INDEX') ?? 0);
    const delegated = config<string>('DELEGATED_SESSION_ID');
    if (delegated) headers['X-Goog-PageId'] = delegated;
  }
  const response = await fetch(`/youtubei/v1${path}?prettyPrint=false`, {
    method: 'POST',
    credentials: 'include',
    headers,
    body: JSON.stringify({ context: config('INNERTUBE_CONTEXT'), ...body }),
  });
  if (!response.ok) {
    throw new Error(`innertube ${path}: HTTP ${response.status}`);
  }
  return (await response.json()) as T;
};
