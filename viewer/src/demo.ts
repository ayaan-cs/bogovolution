// The demo lineage that ships inside the build. Regenerate it with
// `npm run demo` (which runs ../examples/demo/export.sh).
import demo from './demo/tree.json';

export const demoText: string = JSON.stringify(demo);
