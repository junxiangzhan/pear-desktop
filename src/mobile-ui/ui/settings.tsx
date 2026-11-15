// YTM's settings (the account menu's "Settings"), as a page: its categories
// and their items, each saved the way YTM saves it (engine/account.ts).
import {
  createResource,
  createSignal,
  For,
  Match,
  Show,
  Switch,
  untrack,
} from 'solid-js';

import { Icon } from './icons';
import { go, openDialog } from './router';
import { s } from './strings';
import { showToast } from './toast';

import { openUrl, readSettings, runSetting } from '../engine';
import { browseTarget } from '../model/parse';

import type { Confirm, Endpoint, SettingItem } from '../model/types';

type Of<K extends SettingItem['kind']> = Extract<SettingItem, { kind: K }>;

const failed = (error: unknown) => {
  console.error('[pear-mobile-ui]', error);
  showToast(s('actionFailed'));
};

/** Asks first when YTM does (clearing or pausing the history) */
const confirmThen = (confirm: Confirm | undefined, run: () => void) => {
  if (!confirm) run();
  else openDialog({ ...confirm, onConfirm: run });
};

const Toggle = (props: { item: Of<'toggle'> }) => {
  // Each item is rendered once, from the settings as read
  const [on, setOn] = createSignal(untrack(() => props.item.on));
  const flip = () => {
    const next = !on();
    const endpoint = next ? props.item.enable : props.item.disable;
    confirmThen(next ? props.item.confirmOn : props.item.confirmOff, () => {
      setOn(next);
      runSetting(endpoint)
        .then((message) => message && showToast(message))
        .catch((error) => {
          setOn(!next);
          failed(error);
        });
    });
  };
  return (
    <button
      aria-checked={on()}
      class="setting-row"
      onClick={flip}
      role="switch"
    >
      <span class="row-text">
        <span class="title wrap">{props.item.title}</span>
        <Show when={props.item.summary}>
          <span class="subtitle wrap">{props.item.summary}</span>
        </Show>
      </span>
      <span class="switch" classList={{ on: on() }} />
    </button>
  );
};

const Choice = (props: { item: Of<'choice'> }) => {
  const [open, setOpen] = createSignal(false);
  const [selected, setSelected] = createSignal(
    untrack(() => props.item.options.find((option) => option.selected)?.label),
  );
  const pick = (option: Of<'choice'>['options'][number]) => {
    const before = selected();
    setSelected(option.label);
    setOpen(false);
    runSetting(option.endpoint)
      .then((message) => message && showToast(message))
      .catch((error) => {
        setSelected(before);
        failed(error);
      });
  };
  return (
    <>
      <button
        aria-expanded={open()}
        class="setting-row"
        onClick={() => setOpen((shown) => !shown)}
      >
        <span class="row-text">
          <span class="title wrap">{props.item.title}</span>
          <span class="subtitle">{selected()}</span>
        </span>
        <span class="setting-chevron" classList={{ open: open() }}>
          <Icon name="chevron" />
        </span>
      </button>
      <Show when={open()}>
        <div class="setting-options" role="radiogroup">
          <For each={props.item.options}>
            {(option) => (
              <button
                aria-checked={selected() === option.label}
                class="setting-option"
                onClick={() => pick(option)}
                role="radio"
              >
                <span
                  class="menu-mark radio"
                  classList={{ on: selected() === option.label }}
                />
                <span class="row-text">
                  <span class="title wrap">{option.label}</span>
                  <Show when={option.description}>
                    <span class="subtitle wrap">{option.description}</span>
                  </Show>
                </span>
              </button>
            )}
          </For>
        </div>
      </Show>
    </>
  );
};

/** A link: a page of the UI, or one outside the app */
const follow = (endpoint: Endpoint) => {
  const target = browseTarget(endpoint as Parameters<typeof browseTarget>[0]);
  if (target) {
    go({ name: 'browse', target });
    return;
  }
  const url = (endpoint.urlEndpoint as { url?: string } | undefined)?.url;
  if (url) openUrl(url);
};

const Link = (props: { item: Of<'link'> }) => (
  <button
    class="setting-row"
    disabled={!props.item.endpoint}
    onClick={() => props.item.endpoint && follow(props.item.endpoint)}
  >
    <span class="row-text">
      <span class="title wrap">{props.item.title}</span>
      <Show when={props.item.summary}>
        <span class="subtitle wrap">{props.item.summary}</span>
      </Show>
    </span>
  </button>
);

const Action = (props: { item: Of<'action'> }) => (
  <button
    class="setting-row"
    onClick={() =>
      confirmThen(props.item.confirm, () => {
        runSetting(props.item.endpoint)
          .then((message) => message && showToast(message))
          .catch(failed);
      })
    }
  >
    <span class="row-text">
      <span class="title wrap">{props.item.title}</span>
      <Show when={props.item.summary}>
        <span class="subtitle wrap">{props.item.summary}</span>
      </Show>
    </span>
  </button>
);

export const SettingsPage = () => {
  const [categories, { refetch }] = createResource(readSettings);
  return (
    <Switch>
      <Match when={categories.error as unknown}>
        <div class="message">
          <p>{s('error')}</p>
          <button class="tonal-button" onClick={() => refetch()}>
            {s('retry')}
          </button>
        </div>
      </Match>
      <Match when={categories()}>
        {(list) => (
          <div class="settings page-content">
            <For each={list()}>
              {(category) => (
                <section class="setting-category">
                  <h2 class="setting-category-title">{category.title}</h2>
                  <For each={category.items}>
                    {(item) => (
                      <Switch>
                        <Match when={item.kind === 'toggle' && item}>
                          {(toggle) => <Toggle item={toggle()} />}
                        </Match>
                        <Match when={item.kind === 'choice' && item}>
                          {(choice) => <Choice item={choice()} />}
                        </Match>
                        <Match when={item.kind === 'link' && item}>
                          {(link) => <Link item={link()} />}
                        </Match>
                        <Match when={item.kind === 'action' && item}>
                          {(action) => <Action item={action()} />}
                        </Match>
                      </Switch>
                    )}
                  </For>
                </section>
              )}
            </For>
          </div>
        )}
      </Match>
      <Match when={categories.loading}>
        <p class="message">{s('loading')}</p>
      </Match>
    </Switch>
  );
};
