// The account sheet, opened from the avatar: who is signed in, YouTube
// Music's account menu (your channel and history open as the UI's pages;
// settings as a page of its own, switching accounts in this sheet, sign out
// after asking, uploads from a file picker, links in the browser), and
// Pear's app menu (what in-app-menu's title bar offers, which the UI
// replaces on Android).
import {
  createEffect,
  createResource,
  createSignal,
  For,
  Match,
  Show,
  Switch,
} from 'solid-js';

import { Icon, type IconName } from './icons';
import { Presence } from './presence';
import { accountOpen, back, go, openDialog } from './router';
import { swipeDown } from './sheets';
import { s } from './strings';
import { showToast } from './toast';

import {
  account,
  accountAction,
  addAccount,
  type AppMenuItem,
  avatarUrl,
  appMenuVersion,
  clickAppMenu,
  openUrl,
  readAccounts,
  readAppMenu,
  signedIn,
  signIn,
  signOut,
  switchAccount,
  UPLOAD_TYPES,
  uploadSong,
} from '../engine';

import type { AccountItem } from '../model/types';

/** The avatar in the top bar: opens the sheet */
export const AvatarButton = (props: { onClick: () => void }) => (
  <button
    aria-label={s('account')}
    class="icon-button"
    onClick={() => props.onClick()}
  >
    <Show fallback={<Icon name="account" />} when={signedIn() && avatarUrl()}>
      {(url) => <img alt="" class="avatar" src={url()} />}
    </Show>
  </button>
);

/** YTM's icon names (iconType) to the UI's icons */
const ACCOUNT_ICONS: Record<string, IconName> = {
  ACCOUNT_BOX: 'account-box',
  SWITCH_ACCOUNTS: 'switch-account',
  EXIT_TO_APP: 'sign-out',
  CLOUD_UPLOAD: 'upload',
  WATCH_HISTORY: 'history',
  SETTINGS: 'settings',
  PRIVACY_INFO: 'privacy',
  HELP: 'help',
  FEEDBACK: 'feedback',
};

/** The user's songs, from the system's file picker (in the tap itself) */
const pickSongs = () =>
  new Promise<File[]>((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.multiple = true;
    input.accept = UPLOAD_TYPES;
    input.addEventListener('change', () => resolve([...(input.files ?? [])]));
    input.click();
  });

const upload = async (files: File[]) => {
  for (const file of files) {
    showToast(s('uploading', file.name));
    try {
      await uploadSong(file);
      showToast(s('uploaded', file.name));
    } catch (error) {
      console.error('[pear-mobile-ui]', error);
      showToast(s('uploadFailed', file.name));
    }
  }
};

/** Each item as YTM does it (engine/account.ts); `showAccounts` switches */
const runItem = (item: AccountItem, showAccounts: () => void) => {
  const action = accountAction(item);
  switch (action?.kind) {
    case 'page':
      if (item.open) go({ name: 'browse', target: item.open });
      break;
    case 'settings':
      go({ name: 'settings' });
      break;
    case 'accounts':
      showAccounts();
      break;
    case 'signOut':
      back(() =>
        openDialog({
          title: s('signOutTitle', account()?.name ?? ''),
          confirm: s('signOut'),
          onConfirm: signOut,
        }),
      );
      break;
    case 'upload': {
      // The picker opens from the tap (it needs the user's gesture)
      const picked = pickSongs();
      back();
      picked.then(upload).catch(console.error);
      break;
    }
    case 'url':
      openUrl(action.url);
      break;
    default:
      back(() => showToast(s('notSupported')));
  }
};

/** The accounts and channels to switch to, in place of the menu */
const Accounts = (props: { title: string; onBack: () => void }) => {
  const [switcher, { refetch }] = createResource(readAccounts);
  return (
    <>
      <div class="menu-head">
        <button
          aria-label={s('back')}
          class="icon-button"
          onClick={() => props.onBack()}
        >
          <Icon name="back" />
        </button>
        <h2 class="sheet-title">{props.title}</h2>
      </div>
      <Switch>
        <Match when={switcher.error as unknown}>
          <div class="message">
            <p>{s('error')}</p>
            <button class="tonal-button" onClick={() => refetch()}>
              {s('retry')}
            </button>
          </div>
        </Match>
        <Match when={switcher()}>
          {(loaded) => (
            <>
              <Show when={loaded().googleAccount}>
                <div class="sheet-note">{loaded().googleAccount}</div>
              </Show>
              <For each={loaded().accounts}>
                {(entry) => (
                  <button
                    class="row account-row"
                    disabled={entry.selected || !entry.signinUrl}
                    onClick={() => entry.signinUrl && switchAccount(entry.signinUrl)}
                  >
                    <Show
                      fallback={
                        <span class="avatar large icon-thumb">
                          <Icon name="account" />
                        </span>
                      }
                      when={entry.photo}
                    >
                      {(photo) => <img alt="" class="avatar large" src={photo()} />}
                    </Show>
                    <span class="row-text">
                      <span class="title">{entry.name}</span>
                      <span class="subtitle">
                        {[entry.handle, entry.byline].filter(Boolean).join(' · ')}
                      </span>
                    </span>
                    <Show when={entry.selected}>
                      <Icon name="check" />
                    </Show>
                  </button>
                )}
              </For>
              <Show when={loaded().addAccountUrl}>
                {(url) => (
                  <>
                    <hr class="sheet-divider" />
                    <button
                      class="sheet-item with-icon"
                      onClick={() => addAccount(url())}
                    >
                      <span class="menu-icon">
                        <Icon name="add" />
                      </span>
                      <span>{s('addAccount')}</span>
                    </button>
                  </>
                )}
              </Show>
            </>
          )}
        </Match>
        <Match when={switcher.loading}>
          <p class="message">{s('loading')}</p>
        </Match>
      </Switch>
    </>
  );
};

const AccountPhoto = () => (
  <Show
    fallback={
      <span class="avatar large icon-thumb">
        <Icon name="account" />
      </span>
    }
    when={account()?.photo}
  >
    {(photo) => <img alt="" class="avatar large" src={photo()} />}
  </Show>
);

/** Who is signed in (tap: their channel), then YTM's account menu */
const AccountHeader = (props: { onAccounts: (title: string) => void }) => (
  <Show
    fallback={
      <div class="account-head">
        <span class="avatar large icon-thumb">
          <Icon name="account" />
        </span>
        <div class="row-text">
          <div class="subtitle wrap">{s('signInForLibrary')}</div>
        </div>
        <button class="filled-button" onClick={() => back(signIn)}>
          {s('signIn')}
        </button>
      </div>
    }
    when={signedIn()}
  >
    <button
      class="account-head"
      disabled={!account()?.channel}
      onClick={() => {
        const target = account()?.channel;
        if (target) go({ name: 'browse', target });
      }}
    >
      <AccountPhoto />
      <div class="row-text">
        <div class="title">{account()?.name}</div>
        <div class="subtitle">{account()?.handle}</div>
      </div>
      <Show when={account()?.channel}>
        <Icon name="chevron" />
      </Show>
    </button>
    <For each={account()?.sections ?? []}>
      {(section) => (
        <>
          <hr class="sheet-divider" />
          <For each={section}>
            {(item) => (
              <button
                class="sheet-item with-icon"
                onClick={() =>
                  runItem(item, () => props.onAccounts(item.label))
                }
              >
                <span class="menu-icon">
                  <Show when={ACCOUNT_ICONS[item.icon ?? '']}>
                    {(name) => <Icon name={name()} />}
                  </Show>
                </span>
                <span>{item.label}</span>
              </button>
            )}
          </For>
        </>
      )}
    </For>
    <hr class="sheet-divider" />
  </Show>
);

/** One level of the app menu; submenus open below it in the same sheet */
const MenuLevel = (props: {
  items: AppMenuItem[];
  onOpen: (item: AppMenuItem) => void;
  onChanged: () => void;
}) => (
  <For each={props.items.filter((item) => item.visible)}>
    {(item) => (
      <Switch>
        <Match when={item.type === 'separator'}>
          <hr class="sheet-divider" />
        </Match>
        <Match when={item.type === 'submenu'}>
          <button
            class="sheet-item menu-row"
            disabled={!item.enabled}
            onClick={() => props.onOpen(item)}
          >
            <span>{item.label}</span>
            <Icon name="chevron" />
          </button>
        </Match>
        <Match when={item.type === 'checkbox' || item.type === 'radio'}>
          <button
            aria-checked={item.checked}
            class="sheet-item menu-row"
            disabled={!item.enabled}
            onClick={async () => {
              await clickAppMenu(item.commandId);
              props.onChanged();
            }}
            role={item.type === 'radio' ? 'menuitemradio' : 'menuitemcheckbox'}
          >
            <span>{item.label}</span>
            <span
              class="menu-mark"
              classList={{ radio: item.type === 'radio', on: item.checked }}
            />
          </button>
        </Match>
        <Match when={item.type === 'normal'}>
          <button
            class="sheet-item"
            disabled={!item.enabled}
            // Actions (reload, restart, about) act on the app: close first
            onClick={() => back(() => clickAppMenu(item.commandId))}
          >
            {item.label}
          </button>
        </Match>
      </Switch>
    )}
  </For>
);

const AccountSheetBody = () => {
  // Read again whenever Rust rebuilds the menu (after a switch)
  // A failed read is an empty menu: a resource in error throws when read
  const [menu, { refetch }] = createResource(appMenuVersion, () =>
    readAppMenu().catch((error: unknown) => {
      console.error('[pear-mobile-ui]', error);
      return [];
    }),
  );
  // The submenus opened, by label: Rust renumbers the items each time it
  // rebuilds the menu (a plugin switched on gets a submenu)
  const [path, setPath] = createSignal<string[]>([]);
  // The account switcher, in place of the menus (its title: YTM's label)
  const [accounts, setAccounts] = createSignal<string | null>(null);
  // A plugin switched off loses its submenu: back to the deepest level that
  // is still in the rebuilt menu
  createEffect(() => {
    let items = menu() ?? [];
    const open: string[] = [];
    for (const label of path()) {
      const submenu = items.find((entry) => entry.label === label)?.submenu;
      if (!submenu) break;
      open.push(label);
      items = submenu.items;
    }
    if (menu() && open.length < path().length) setPath(open);
  });
  const level = () => {
    let items = menu() ?? [];
    let title: string | undefined;
    for (const label of path()) {
      const item = items.find((entry) => entry.label === label);
      items = item?.submenu?.items ?? [];
      title = item?.label;
    }
    return { items, title };
  };
  return (
    <>
      <div class="scrim" onClick={() => back()} />
      <div class="sheet tall account-sheet">
        <div class="sheet-handle" {...swipeDown(() => back())} />
        <div class="sheet-scroll">
          <Show when={accounts()}>
            {(title) => (
              <Accounts onBack={() => setAccounts(null)} title={title()} />
            )}
          </Show>
          <Show when={!accounts()}>
          <Show
            fallback={
              <>
                <AccountHeader onAccounts={setAccounts} />
                <h2 class="sheet-title">{s('appMenu')}</h2>
              </>
            }
            when={path().length}
          >
            <div class="menu-head">
              <button
                aria-label={s('back')}
                class="icon-button"
                onClick={() => setPath((open) => open.slice(0, -1))}
              >
                <Icon name="back" />
              </button>
              <h2 class="sheet-title">{level().title}</h2>
            </div>
          </Show>
          <Show
            fallback={<p class="message">{s('loading')}</p>}
            when={!menu.loading || menu()}
          >
            <MenuLevel
              items={level().items}
              // Labels and checks change with each switch (a plugin
              // turned on gets its own submenu): read the tree again
              onChanged={() => refetch()}
              onOpen={(item) => setPath((open) => [...open, item.label])}
            />
          </Show>
          </Show>
        </div>
      </div>
    </>
  );
};

export const AccountSheet = () => (
  <Presence when={accountOpen()}>{() => <AccountSheetBody />}</Presence>
);
