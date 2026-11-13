// Order matters: the shim defines the globals that plugin modules and the
// renderer read at evaluation time, so it has to come first.
import './shim';
// GeckoView: a tap's trailing click must not land on what it just opened
import './ghost-clicks';
// Before YTM's scripts run (it isolates their history handling); a no-op on
// desktop
import './mobile-ui';
import './plugin-stubs';
import './preload-plugins';
import './menu-runtime';
import './plugin-glue';
import '@/renderer';
