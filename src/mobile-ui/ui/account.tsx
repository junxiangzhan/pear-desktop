// The account sheet, opened from the avatar: who is signed in, YTM's own
// account menu (its items drawn here: your channel and history open as the
// new UI's pages, the rest runs through YTM), and Pear's app menu (what in-app-menu's title bar offers,
// which the new UI replaces on Android).
import {
  createResource,
  createSignal,
  For,
  Match,
  Show,
  Switch,
} from 'solid-js';

import { Icon, type IconName } from './icons';
import { accountOpen, back, go, openAccount } from './router';
import { swipeDown } from './sheets';
import { s } from './strings';

import {
  account,
  type AppMenuItem,
  avatarUrl,
  clickAppMenu,
  readAppMenu,
  runAccountItem,
  signedIn,
  signIn,
} from '../engine/ytm';

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

/** Pages open in the new UI; the rest is YTM's (dialogs, sign out) */
const runItem = (item: AccountItem) => {
  const target = item.open;
  if (target) go({ name: 'browse', target });
  // The account switcher's ← comes back to this sheet
  else back(() => runAccountItem(item, openAccount));
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
const AccountHeader = () => (
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
                onClick={() => runItem(item)}
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
  const [menu, { refetch }] = createResource(readAppMenu);
  // The submenus opened, by label: Rust renumbers the items each time it
  // rebuilds the menu (a plugin switched on gets a submenu)
  const [path, setPath] = createSignal<string[]>([]);
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
          <Show
            fallback={
              <>
                <AccountHeader />
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
        </div>
      </div>
    </>
  );
};

export const AccountSheet = () => (
  <Show when={accountOpen()}>
    <AccountSheetBody />
  </Show>
);
