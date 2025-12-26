<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';

  import { onMount } from 'svelte';

  onMount(() => {
    document.addEventListener('contextmenu', (e) => e.preventDefault());
  });

  interface FileInfo {
    path: string;
    name: string;
    vertices: number;
    faces: number;
    voxel_count: number;
    cube_count: number;
  }

  interface ConvertResult {
    success: boolean;
    message: string;
    output_path: string | null;
    voxel_count: number;
    cube_count: number;
  }

  type Lang = 'en' | 'ru' | 'ja';

  const translations: Record<Lang, Record<string, string>> = {
    en: {
      voxels: 'Voxels',
      cubes: 'cubes',
      output: 'Output',
      select: 'Select...',
      convert: 'Convert',
      converting: 'Converting...',
      done: 'done',
      files: 'Files',
      add: 'Add',
      dropFiles: 'Drop OBJ files here'
    },
    ru: {
      voxels: 'Воксели',
      cubes: 'кубов',
      output: 'Вывод',
      select: 'Выбрать...',
      convert: 'Конвертировать',
      converting: 'Конвертация...',
      done: 'готово',
      files: 'Файлы',
      add: 'Добавить',
      dropFiles: 'Перетащите OBJ файлы сюда'
    },
    ja: {
      voxels: 'ボクセル',
      cubes: 'キューブ',
      output: '出力先',
      select: '選択...',
      convert: '変換',
      converting: '変換中...',
      done: '完了',
      files: 'ファイル',
      add: '追加',
      dropFiles: 'OBJファイルをここにドロップ'
    }
  };

  const langNames: Record<Lang, string> = { en: 'EN', ru: 'RU', ja: 'JA' };
  const langs: Lang[] = ['en', 'ru', 'ja'];

  let lang: Lang = (typeof localStorage !== 'undefined' && localStorage.getItem('lang') as Lang) || 'en';
  $: t = translations[lang];

  function cycleLang() {
    const idx = langs.indexOf(lang);
    lang = langs[(idx + 1) % langs.length];
    localStorage.setItem('lang', lang);
  }

  let files: FileInfo[] = [];
  let scale = 16;
  let outputDir = '';
  let converting = false;
  let analyzing = false;
  let results: ConvertResult[] = [];

  async function selectFiles() {
    const selected = await open({
      multiple: true,
      filters: [{ name: 'OBJ Files', extensions: ['obj'] }]
    });
    if (selected) {
      const paths = Array.isArray(selected) ? selected : [selected];
      analyzing = true;
      for (const path of paths) {
        if (!files.find(f => f.path === path)) {
          try {
            const info: FileInfo = await invoke('analyze_file', { path, scale: scale as number });
            files = [...files, info];
          } catch (e) {
            console.error(e);
          }
        }
      }
      analyzing = false;
    }
  }

  async function selectOutputDir() {
    const selected = await open({ directory: true });
    if (selected && typeof selected === 'string') {
      outputDir = selected;
    }
  }

  function removeFile(path: string) {
    files = files.filter(f => f.path !== path);
  }

  async function updateEstimates() {
    if (files.length === 0) return;
    analyzing = true;
    const updated: FileInfo[] = [];
    for (const file of files) {
      try {
        const info: FileInfo = await invoke('analyze_file', { path: file.path, scale: scale as number });
        updated.push(info);
      } catch {
        updated.push(file);
      }
    }
    files = updated;
    analyzing = false;
  }

  async function convertAll() {
    if (!outputDir || files.length === 0) return;
    converting = true;
    results = [];
    for (const file of files) {
      try {
        const result: ConvertResult = await invoke('convert_file', {
          path: file.path,
          outputDir,
          scale: scale as number
        });
        results = [...results, result];
      } catch (e) {
        results = [...results, {
          success: false,
          message: String(e),
          output_path: null,
          voxel_count: 0,
          cube_count: 0
        }];
      }
    }
    converting = false;
  }

  function totalCubes(): number {
    return files.reduce((sum, f) => sum + f.cube_count, 0);
  }

  function fmt(n: number): string {
    return n.toLocaleString();
  }
</script>

<div class="app">
  <aside class="sidebar">
    <div class="header">
      <div class="logo">OBJ2MC</div>
      <button class="lang-btn" on:click={cycleLang}>{langNames[lang]}</button>
    </div>
    
    <div class="settings">
      <div class="field">
        <span class="field-label">{t.voxels}</span>
        <div class="field-input">
          <input type="number" bind:value={scale} min="1" max="128" on:change={updateEstimates} />
        </div>
        <span class="field-hint">{fmt(totalCubes())} {t.cubes}</span>
      </div>
      
      <div class="field">
        <span class="field-label">{t.output}</span>
        <button class="folder-btn" on:click={selectOutputDir}>
          {#if outputDir}
            {outputDir.split(/[/\\]/).pop()}
          {:else}
            {t.select}
          {/if}
        </button>
      </div>
    </div>

    <button 
      class="convert-btn" 
      on:click={convertAll} 
      disabled={!outputDir || files.length === 0 || converting || analyzing}
    >
      {#if converting}
        {t.converting}
      {:else}
        {t.convert}
      {/if}
    </button>

    {#if results.length > 0}
      <div class="status">
        {results.filter(r => r.success).length}/{results.length} {t.done}
      </div>
    {/if}
  </aside>

  <main class="content">
    <div class="toolbar">
      <span class="toolbar-title">{t.files} ({files.length})</span>
      {#if files.length > 0}
        <span class="toolbar-info">{fmt(totalCubes())} {t.cubes}</span>
      {/if}
      <button class="toolbar-btn" on:click={selectFiles}>{t.add}</button>
    </div>

    <div class="file-list">
      {#if files.length === 0}
        <button class="empty-state" on:click={selectFiles}>
          <span>{t.dropFiles}</span>
        </button>
      {:else}
        {#each files as file, i}
          <div class="file-item" class:done={results[i]?.success} class:error={results[i] && !results[i].success}>
            <div class="file-main">
              <span class="file-name">{file.name}</span>
              {#if results[i]}
                <span class="file-result">{results[i].message}</span>
              {/if}
            </div>
            <div class="file-stats">
              <span>{fmt(file.vertices)}v</span>
              <span>{fmt(file.faces)}f</span>
              <span class="cubes">{analyzing ? '...' : fmt(file.cube_count)}</span>
            </div>
            <button class="file-remove" on:click={() => removeFile(file.path)}>×</button>
          </div>
        {/each}
      {/if}
    </div>
  </main>
</div>


<style>
  :global(*) {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
  }

  :global(body) {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Hiragino Sans', 'Meiryo', sans-serif;
    background: #0d1117;
    color: #e6edf3;
    font-size: 13px;
    overflow: hidden;
    user-select: none;
  }

  :global(::-webkit-scrollbar) {
    width: 8px;
  }

  :global(::-webkit-scrollbar-track) {
    background: transparent;
  }

  :global(::-webkit-scrollbar-thumb) {
    background: #30363d;
    border-radius: 4px;
  }

  :global(::-webkit-scrollbar-thumb:hover) {
    background: #484f58;
  }

  .app {
    display: flex;
    height: 100vh;
  }

  .sidebar {
    width: 180px;
    background: #161b22;
    border-right: 1px solid #21262d;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding-bottom: 12px;
    border-bottom: 1px solid #21262d;
  }

  .logo {
    font-size: 15px;
    font-weight: 600;
    color: #f0f6fc;
  }

  .lang-btn {
    padding: 2px 6px;
    background: #21262d;
    border: 1px solid #30363d;
    border-radius: 4px;
    color: #7d8590;
    font-size: 10px;
    font-weight: 600;
    cursor: pointer;
  }

  .lang-btn:hover {
    color: #e6edf3;
    border-color: #484f58;
  }

  .settings {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .field-label {
    font-size: 11px;
    color: #7d8590;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .field-input {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .field-input input {
    width: 60px;
    padding: 6px 8px;
    background: #0d1117;
    border: 1px solid #30363d;
    border-radius: 6px;
    color: #e6edf3;
    font-size: 13px;
  }

  .field-input input:focus {
    outline: none;
    border-color: #58a6ff;
  }

  .field-hint {
    font-size: 11px;
    color: #3fb950;
    margin-top: 4px;
  }

  .folder-btn {
    width: 100%;
    padding: 6px 10px;
    background: #21262d;
    border: 1px solid #30363d;
    border-radius: 6px;
    color: #7d8590;
    font-size: 12px;
    cursor: pointer;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder-btn:hover {
    border-color: #484f58;
  }

  .convert-btn {
    margin-top: auto;
    padding: 10px;
    background: #238636;
    border: none;
    border-radius: 6px;
    color: #fff;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
  }

  .convert-btn:hover:not(:disabled) {
    background: #2ea043;
  }

  .convert-btn:disabled {
    background: #21262d;
    color: #484f58;
    cursor: not-allowed;
  }

  .status {
    font-size: 11px;
    color: #7d8590;
    text-align: center;
  }

  .content {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 16px;
    background: #161b22;
    border-bottom: 1px solid #21262d;
  }

  .toolbar-title {
    font-weight: 500;
    color: #e6edf3;
  }

  .toolbar-info {
    color: #7d8590;
    font-size: 12px;
  }

  .toolbar-btn {
    margin-left: auto;
    padding: 5px 12px;
    background: #21262d;
    border: 1px solid #30363d;
    border-radius: 6px;
    color: #c9d1d9;
    font-size: 12px;
    cursor: pointer;
  }

  .toolbar-btn:hover {
    background: #30363d;
  }

  .file-list {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
  }

  .empty-state {
    width: 100%;
    height: 100%;
    min-height: 200px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px dashed #30363d;
    border-radius: 6px;
    color: #484f58;
    font-size: 13px;
    cursor: pointer;
  }

  .empty-state:hover {
    border-color: #484f58;
    color: #7d8590;
  }

  .file-item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    background: #161b22;
    border: 1px solid #21262d;
    border-radius: 6px;
    margin-bottom: 6px;
  }

  .file-item.done {
    border-color: #238636;
  }

  .file-item.error {
    border-color: #f85149;
  }

  .file-main {
    flex: 1;
    min-width: 0;
  }

  .file-name {
    display: block;
    font-weight: 500;
    color: #e6edf3;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .file-result {
    display: block;
    font-size: 11px;
    color: #7d8590;
    margin-top: 2px;
  }

  .file-item.done .file-result {
    color: #3fb950;
  }

  .file-item.error .file-result {
    color: #f85149;
  }

  .file-stats {
    display: flex;
    gap: 8px;
    font-size: 11px;
    color: #7d8590;
  }

  .file-stats .cubes {
    color: #3fb950;
    font-weight: 500;
  }

  .file-remove {
    background: none;
    border: none;
    color: #484f58;
    font-size: 16px;
    cursor: pointer;
    padding: 2px 6px;
  }

  .file-remove:hover {
    color: #f85149;
  }
</style>
