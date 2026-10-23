// Order matters: the shim defines the globals that plugin modules and the
// renderer read at evaluation time, so it has to come first.
import './shim';
import './plugin-stubs';
import '@/renderer';
