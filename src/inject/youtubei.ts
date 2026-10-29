// youtubei.js in the page, for the backends that need it (crossfade's
// `audio-url`, the downloader): src/plugins/utils/main/youtubei.ts without
// happy-dom or node:vm. youtubei.js needs a JavaScript evaluator and
// BotGuard for the PO token, which Rust cannot provide but a real browser
// page can.
//
// Requests to this origin (the YTMUSIC client's Innertube API) use the page's
// own `fetch` with its cookies, like Electron's session-bound `net.fetch`;
// www.youtube.com (player script, BotGuard) goes through Rust.
import {
  BG,
  base64ToU8,
  buildURL,
  getHeaders,
  u8ToBase64,
  type WebPoSignalOutput,
} from 'bgutils-js';
import { Innertube, Platform, type Types } from 'youtubei.js';

const requestKey = 'O43z0dpjhgX20SCx4KAo';

const scriptPolicy = window.trustedTypes?.createPolicy('pear-tauri-youtubei', {
  createScript: (script: string) => script,
});

const pageFetch: typeof fetch = (input, init) => {
  const url = new URL(input instanceof Request ? input.url : String(input));
  return url.origin === location.origin
    ? window.fetch(input, init)
    : window.__pearCrossOriginFetch!(input, init);
};

// Same evaluator as `setupYoutubeiEvaluator`, through a Trusted Types policy
Platform.shim.eval = (
  data: Types.BuildScriptResult,
  env: Record<string, Types.VMPrimative>,
) => {
  const properties = [];
  if (env.n) properties.push(`n: exportedVars.nFunction("${env.n}")`);
  if (env.sig) properties.push(`sig: exportedVars.sigFunction("${env.sig}")`);
  const code = `${data.output}\nreturn { ${properties.join(', ')} }`;
  // oxlint-disable-next-line typescript/no-implied-eval, typescript/no-unsafe-call
  return new Function(
    (scriptPolicy?.createScript(code) ?? code) as string,
  )() as never;
};

interface PoTokenMinter {
  expiresAt: number;
  mint: (identifier: string) => Promise<string>;
  close: () => void;
}

// BotGuard runs in a hidden same-origin frame so its globals cannot collide
// with the page's own BotGuard
const createPoTokenMinter = async (
  visitorData: string,
): Promise<PoTokenMinter> => {
  const frame = document.createElement('iframe');
  frame.style.display = 'none';
  document.documentElement.append(frame);
  const realm = frame.contentWindow as Window & typeof globalThis;
  try {
    const challenge = await BG.Challenge.create({
      fetch: pageFetch,
      globalObj: realm,
      identifier: visitorData,
      requestKey,
    });
    const interpreter =
      challenge?.interpreterJavascript
        .privateDoNotAccessOrElseSafeScriptWrappedValue;
    if (!challenge || !interpreter)
      throw new Error('Could not get the BotGuard challenge');

    const realmPolicy = realm.trustedTypes?.createPolicy(
      'pear-tauri-botguard',
      {
        createScript: (script: string) => script,
      },
    );
    realm.eval(
      (realmPolicy?.createScript(interpreter) ?? interpreter) as string,
    );
    const botguard = await BG.BotGuardClient.create({
      program: challenge.program,
      globalName: challenge.globalName,
      globalObj: realm,
    });
    // Must come from BotGuard's realm, it only fills arrays of its own realm
    const webPoSignalOutput = new realm.Array() as WebPoSignalOutput;
    const botguardResponse = await botguard.snapshot({ webPoSignalOutput });
    const response = await pageFetch(buildURL('GenerateIT'), {
      method: 'POST',
      headers: getHeaders(),
      body: JSON.stringify([requestKey, botguardResponse]),
    });
    const [integrityToken, estimatedTtlSecs] = (await response.json()) as [
      string,
      number,
    ];

    const mintCallback = await webPoSignalOutput[0]?.(
      base64ToU8(integrityToken),
    );
    if (typeof mintCallback !== 'function')
      throw new Error('Could not create the PO token minter');

    const ttlMs = estimatedTtlSecs * 1000;
    return {
      expiresAt: Date.now() + ttlMs,
      async mint(identifier) {
        const token = await mintCallback(new TextEncoder().encode(identifier));
        if (!token) throw new Error('Could not mint a PO token');
        return u8ToBase64(new Uint8Array(token), true);
      },
      close: () => frame.remove(),
    };
  } catch (error) {
    frame.remove();
    throw error;
  }
};

let cachedMinter:
  | { visitorData: string; minter: Promise<PoTokenMinter> }
  | undefined;

// Same caching and expiry as `mintPoToken` in src/plugins/utils/main/youtubei.ts
const mintPoToken = async (
  visitorData: string,
  identifier: string,
): Promise<string> => {
  if (cachedMinter?.visitorData !== visitorData) {
    cachedMinter?.minter.then(
      (minter) => minter.close(),
      () => {},
    );
    cachedMinter = { visitorData, minter: createPoTokenMinter(visitorData) };
  }
  const current = cachedMinter;
  let minter: PoTokenMinter;
  try {
    minter = await current.minter;
  } catch (error) {
    if (cachedMinter === current) cachedMinter = undefined;
    throw error;
  }
  if (Date.now() < minter.expiresAt) return await minter.mint(identifier);
  if (cachedMinter === current) cachedMinter = undefined;
  minter.close();
  return await mintPoToken(visitorData, identifier);
};

let innertube: Promise<Innertube> | undefined;

/** One Innertube session for the page, created on first use. */
export const getInnertube = async (): Promise<Innertube> => {
  innertube ??= Innertube.create({ fetch: pageFetch });
  try {
    return await innertube;
  } catch (error) {
    innertube = undefined;
    throw error;
  }
};

/** A PO token bound to `videoId`, for `po_token` and the stream's `pot`. */
export const poTokenFor = async (yt: Innertube, videoId: string) =>
  await mintPoToken(yt.session.context.client.visitorData ?? '', videoId);
