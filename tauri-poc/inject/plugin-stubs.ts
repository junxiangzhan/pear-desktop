// Imported after `./shim`, so every plugin module evaluates with the shimmed
// globals already defined.
import { allPlugins } from 'virtual:plugins';

import { providePluginStubs } from './shim';

void providePluginStubs(allPlugins);
