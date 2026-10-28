// Prompt dialogs (input, select, counter, keybind, multi-input), rendered by
// the Rust prompt window (tauri-poc/src-tauri/src/prompt.rs,
// tauri-poc/placeholder/prompt.html). Same options and results as
// custom-electron-prompt, which plugin menus were written against; resolves
// to null when cancelled.
import { invoke } from './tauri';

export type SelectOptions = Record<string, string>;

export interface CounterOptions {
  minimum?: number;
  maximum?: number;
  multiFire?: boolean;
}

export interface KeybindOptions {
  value: string;
  label: string;
  default?: string;
}

export interface InputOptions {
  label: string;
  value: unknown;
  inputAttrs?: Partial<HTMLInputElement>;
  selectOptions?: SelectOptions;
}

interface BasePromptOptions<T extends string> {
  type?: T;
  width?: number;
  height?: number;
  resizable?: boolean;
  title?: string;
  label?: string;
  buttonLabels?: {
    ok?: string;
    cancel?: string;
  };
  alwaysOnTop?: boolean;
  value?: unknown;
  useHtmlLabel?: boolean;
  skipTaskbar?: boolean;
  inputAttrs?: Partial<HTMLInputElement>;
}

export type InputPromptOptions = BasePromptOptions<'input'>;
export interface SelectPromptOptions extends BasePromptOptions<'select'> {
  selectOptions: SelectOptions;
}
export interface CounterPromptOptions extends BasePromptOptions<'counter'> {
  counterOptions: CounterOptions;
}
export interface MultiInputPromptOptions extends BasePromptOptions<'multiInput'> {
  multiInputOptions: InputOptions[];
}
export interface KeybindPromptOptions extends BasePromptOptions<'keybind'> {
  keybindOptions: KeybindOptions[];
}

type PromptType = 'input' | 'select' | 'counter' | 'keybind' | 'multiInput';

export type PromptOptions<T extends PromptType> = T extends 'input'
  ? InputPromptOptions
  : T extends 'select'
    ? SelectPromptOptions
    : T extends 'counter'
      ? CounterPromptOptions
      : T extends 'keybind'
        ? KeybindPromptOptions
        : MultiInputPromptOptions;

export type PromptResult<T extends PromptType> = T extends 'input' | 'select'
  ? string
  : T extends 'counter'
    ? number
    : T extends 'keybind'
      ? { value: string; accelerator: string }[]
      : string[];

const prompt = async <T extends PromptType>(
  options: PromptOptions<T> & { type: T },
): Promise<PromptResult<T> | null> =>
  (await invoke('prompt_open', { options })) as PromptResult<T> | null;

export default prompt;
