<script lang="ts">
  import { html } from 'diff2html';
  import { ColorSchemeType } from 'diff2html/lib/types';
  import 'diff2html/bundles/css/diff2html.min.css';
  import type { Theme } from '../lib/theme';

  let { diff, theme }: { diff: string; theme: Theme } = $props();

  const rendered = $derived(
    html(diff, {
      drawFileList: false,
      matching: 'lines',
      outputFormat: 'line-by-line',
      colorScheme: theme === 'dark' ? ColorSchemeType.DARK : ColorSchemeType.LIGHT,
    }),
  );
</script>

<div class="diff" aria-label="Changes from the parent version">
  {@html rendered}
</div>

<style>
  .diff {
    border-radius: 8px;
    overflow: auto;
    max-height: 22rem;
    border: 1px solid var(--line);
    position: relative;
  }
  .diff :global(.d2h-file-diff) {
    position: relative;
  }
  .diff :global(.d2h-file-wrapper) {
    border: none;
    margin-bottom: 0;
  }
</style>
