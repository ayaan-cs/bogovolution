<script lang="ts">
  let {
    title,
    message,
    details,
    ondismiss,
  }: { title: string; message: string; details: string[]; ondismiss: () => void } = $props();
</script>

<div class="banner card" role="alert">
  <div class="top">
    <strong>{title}</strong>
    <button class="btn" onclick={ondismiss}>Dismiss</button>
  </div>
  <p>{message}</p>
  {#if details.length > 0}
    <ul>
      {#each details as d}
        <li class="mono">{d}</li>
      {/each}
    </ul>
  {/if}
  <p class="hint">The lineage that was already open is still shown below.</p>
</div>

<style>
  .banner {
    border-color: var(--c-rejected);
    background: var(--bg-rejected);
    padding: 0.9rem 1.1rem;
    margin-bottom: 1rem;
  }
  .top {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 1rem;
  }
  p {
    margin: 0.4rem 0;
  }
  ul {
    margin: 0.4rem 0;
    padding-left: 1.2rem;
    font-size: 0.85rem;
  }
  .hint {
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
