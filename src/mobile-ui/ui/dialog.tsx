// A question before an action (sign out, clear the history): a card over a
// scrim; the back button, the scrim and Cancel close it.
import { Show } from 'solid-js';

import { Presence } from './presence';
import { back, dialog } from './router';
import { s } from './strings';

export const DialogView = () => (
  <Presence when={dialog()}>
    {(shown) => (
      <>
        <div class="scrim" onClick={() => back()} />
        <div aria-modal="true" class="dialog" role="alertdialog">
          <h2 class="dialog-title">{shown.title}</h2>
          <Show when={shown.message}>
            <p class="dialog-message">{shown.message}</p>
          </Show>
          <div class="dialog-buttons">
            <button class="text-button" onClick={() => back()}>
              {shown.cancel ?? s('cancel')}
            </button>
            <button class="text-button" onClick={() => back(shown.onConfirm)}>
              {shown.confirm}
            </button>
          </div>
        </div>
      </>
    )}
  </Presence>
);
