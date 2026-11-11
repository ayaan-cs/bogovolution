<script lang="ts">
  import { onMount } from 'svelte';
  import Charts from './components/Charts.svelte';
  import Details from './components/Details.svelte';
  import ErrorBanner from './components/ErrorBanner.svelte';
  import Footer from './components/Footer.svelte';
  import Graph from './components/Graph.svelte';
  import { demoText } from './demo';
  import { pct, resolvePrefix, shortHash } from './lib/format';
  import { applyTheme, initialTheme, type Theme } from './lib/theme';
  import type { LineageExport, Selection } from './lib/types';
  import { parseExport, type ParseResult } from './lib/validate';

  type Failure = Extract<ParseResult, { ok: false }>;

  const first = parseExport(demoText);
  if (!first.ok) throw new Error('The bundled demo export is invalid: ' + first.title);

  let data = $state<LineageExport>(first.data);
  let source = $state('Bundled demo');
  let isDemo = $state(true);
  let error = $state<Failure | null>(null);
  let selection = $state<Selection | null>(null);
  // Set the colors before any child draws, so canvas and chart colors are right on first paint.
  const startTheme = initialTheme();
  applyTheme(startTheme);
  let theme = $state<Theme>(startTheme);
  let dragging = $state(false);
  let busy = $state(false);
  let fileInput: HTMLInputElement;

  const current = $derived(data.nodes.find((n) => n.hash === data.current));
  const root = $derived(data.nodes.find((n) => n.hash === data.root));

  function defaultSelection(d: LineageExport): Selection {
    return { kind: 'node', hash: d.current };
  }

  function fromHash(d: LineageExport): Selection | null {
    const m = /^#v=([0-9a-f]{4,64})$/.exec(location.hash);
    const hit = m ? resolvePrefix(d, m[1]) : undefined;
    return hit ? { kind: 'node', hash: hit.hash } : null;
  }

  function select(s: Selection) {
    selection = s;
    if (s.kind === 'node') history.replaceState(null, '', `#v=${shortHash(s.hash, 12)}`);
    else history.replaceState(null, '', location.pathname + location.search);
  }

  function accept(result: ParseResult, name: string, demo = false) {
    if (!result.ok) {
      error = result;
      return;
    }
    data = result.data;
    source = name;
    isDemo = demo;
    error = null;
    selection = fromHash(result.data) ?? defaultSelection(result.data);
  }

  async function loadFile(file: File) {
    busy = true;
    try {
      accept(parseExport(await file.text()), file.name);
    } catch {
      error = { ok: false, title: 'Could not read that file', message: 'The browser could not read it. Try saving it again.', details: [] };
    } finally {
      busy = false;
    }
  }

  async function loadUrl(url: string) {
    busy = true;
    try {
      const res = await fetch(url);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      accept(parseExport(await res.text()), url);
    } catch (err) {
      error = {
        ok: false,
        title: 'Could not load that URL',
        message:
          'Fetching a file by URL needs the page to be served over http. If you opened this page straight from disk, use Open file or drag the file onto the page instead.',
        details: [`${url}: ${err instanceof Error ? err.message : String(err)}`],
      };
    } finally {
      busy = false;
    }
  }

  function onPick(e: Event) {
    const f = (e.currentTarget as HTMLInputElement).files?.[0];
    if (f) void loadFile(f);
    (e.currentTarget as HTMLInputElement).value = '';
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    const f = e.dataTransfer?.files?.[0];
    if (f) void loadFile(f);
  }

  function toggleTheme() {
    theme = theme === 'dark' ? 'light' : 'dark';
  }

  $effect(() => {
    applyTheme(theme);
  });

  onMount(() => {
    selection = fromHash(data) ?? defaultSelection(data);
    const url = new URLSearchParams(location.search).get('tree');
    if (url) void loadUrl(url);
  });
</script>

<svelte:window
  ondragover={(e) => {
    e.preventDefault();
    dragging = true;
  }}
  ondragleave={(e) => {
    if (e.relatedTarget === null) dragging = false;
  }}
  ondrop={onDrop}
/>

<div class="page">
  <header class="top">
    <div class="brand">
      <svg width="28" height="28" viewBox="0 0 16 16" aria-hidden="true">
        <path d="M4 4 L12 8 M4 4 L4 12" stroke="var(--muted)" stroke-width="1" fill="none" />
        <circle cx="4" cy="4" r="2.2" fill="var(--c-accepted)" />
        <circle cx="12" cy="8" r="2.2" fill="var(--c-rejected)" />
        <circle cx="4" cy="12" r="2.2" fill="var(--c-noise)" />
      </svg>
      <div>
        <h1>rules-evolve lineage</h1>
        <p class="sub">
          <code>{data.project.rules_file}</code> · model <code>{data.project.model}</code> · {source}
          {#if isDemo}<span class="badge pending">demo</span>{/if}
        </p>
      </div>
    </div>
    <div class="actions">
      <input bind:this={fileInput} type="file" accept=".json,application/json" onchange={onPick} hidden />
      <button class="btn primary" onclick={() => fileInput.click()} disabled={busy}>Open tree.json</button>
      <button class="btn" onclick={toggleTheme} aria-label="Switch to {theme === 'dark' ? 'light' : 'dark'} theme">
        {theme === 'dark' ? 'Light' : 'Dark'} theme
      </button>
    </div>
  </header>

  <main>
    {#if error}
      <ErrorBanner title={error.title} message={error.message} details={error.details} ondismiss={() => (error = null)} />
    {/if}

    <section class="strip" aria-label="Summary">
      <div class="stat"><span class="n">{data.summary.nodes}</span><span class="l">versions</span></div>
      <div class="stat accepted"><span class="n">{data.summary.accepted}</span><span class="l">accepted</span></div>
      <div class="stat rejected"><span class="n">{data.summary.rejected}</span><span class="l">rejected</span></div>
      <div class="stat noise"><span class="n">{data.summary.noise}</span><span class="l">noise</span></div>
      <div class="stat pending"><span class="n">{data.summary.pending}</span><span class="l">pending</span></div>
      <div class="stat refused"><span class="n">{data.summary.refused}</span><span class="l">refused</span></div>
      <div class="stat wide">
        <span class="n">{pct(root?.median_pass_rate)} to {pct(current?.median_pass_rate)}</span>
        <span class="l">median pass rate, root to current</span>
      </div>
      <div class="stat wide">
        <span class="n mono">{data.chain.head_hash.slice(0, 12)}</span>
        <span class="l">hash chain head at export, {data.chain.events} events</span>
      </div>
    </section>

    <section class="stage">
      <div class="card graphcard"><Graph {data} {selection} {theme} onselect={select} /></div>
      <Details {data} {selection} {theme} onselect={select} />
    </section>

    <Charts {data} {selection} {theme} />
  </main>

  <Footer />

  {#if dragging}
    <div class="drop" aria-hidden="true"><div class="card">Drop a tree.json to view it</div></div>
  {/if}
</div>

<style>
  .page {
    min-height: 100vh;
  }
  .top {
    max-width: 1500px;
    margin: 0 auto;
    padding: 1.1rem 1.25rem 0.6rem;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    flex-wrap: wrap;
  }
  .brand {
    display: flex;
    gap: 0.75rem;
    align-items: center;
  }
  h1 {
    margin: 0;
    font-size: 1.25rem;
    letter-spacing: -0.01em;
  }
  .sub {
    margin: 0;
    color: var(--muted);
    font-size: 0.85rem;
  }
  .actions {
    display: flex;
    gap: 0.5rem;
  }
  main {
    max-width: 1500px;
    margin: 0 auto;
    padding: 0.5rem 1.25rem 0;
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .strip {
    display: flex;
    gap: 0.6rem;
    flex-wrap: wrap;
  }
  .stat {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 0.4rem 0.85rem;
    display: flex;
    flex-direction: column;
    min-width: 5.2rem;
  }
  .stat.wide {
    min-width: 12rem;
  }
  .n {
    font-size: 1.15rem;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .l {
    font-size: 0.72rem;
    color: var(--muted);
  }
  .stat.accepted .n { color: var(--c-accepted); }
  .stat.rejected .n { color: var(--c-rejected); }
  .stat.noise .n { color: var(--c-noise); }
  .stat.pending .n { color: var(--c-pending); }
  .stat.refused .n { color: var(--c-refused); }
  .stage {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(340px, 460px);
    gap: 1rem;
    height: 720px;
  }
  .graphcard {
    overflow: hidden;
    min-width: 0;
  }
  .stage > :global(aside) {
    min-height: 0;
  }
  .drop {
    position: fixed;
    inset: 0;
    background: color-mix(in srgb, var(--bg) 70%, transparent);
    display: grid;
    place-items: center;
    z-index: 10;
    pointer-events: none;
  }
  .drop .card {
    padding: 2rem 3rem;
    font-size: 1.2rem;
    border: 2px dashed var(--accent);
  }
  @media (max-width: 960px) {
    .stage {
      grid-template-columns: 1fr;
      height: auto;
    }
    .graphcard {
      height: 480px;
    }
  }
</style>
