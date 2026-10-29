// Order matters: the shim defines the globals that plugin modules and the
// renderer read at evaluation time, so it has to come first.
import './shim';
import './plugin-stubs';
import './preload-plugins';
import './menu-runtime';
import './plugin-glue';
import '@/renderer';
