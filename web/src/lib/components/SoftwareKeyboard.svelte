<script lang="ts">
  import type { ApplicationRuntime } from '../runtime';

  interface Props {
    runtime: ApplicationRuntime;
    language: 'ja' | 'en';
  }

  let { runtime, language }: Props = $props();
  let open = $state(false);
  let text = $state('');
  let busy = $state(false);

  const englishRows = ['1234567890', 'qwertyuiop', 'asdfghjkl', 'zxcvbnm'] as const;
  const kanaRows = ['あかさたなはまやらわ', 'いきしちにひみりを', 'うくすつぬふむゆるん', 'えけせてねへめよれ', 'おこそとのほもろ'] as const;

  const kanaToRomaji: Readonly<Record<string, string>> = {
    あ: 'a', い: 'i', う: 'u', え: 'e', お: 'o',
    か: 'ka', き: 'ki', く: 'ku', け: 'ke', こ: 'ko',
    が: 'ga', ぎ: 'gi', ぐ: 'gu', げ: 'ge', ご: 'go',
    さ: 'sa', し: 'shi', す: 'su', せ: 'se', そ: 'so',
    ざ: 'za', じ: 'ji', ず: 'zu', ぜ: 'ze', ぞ: 'zo',
    た: 'ta', ち: 'chi', つ: 'tsu', て: 'te', と: 'to',
    だ: 'da', ぢ: 'di', づ: 'du', で: 'de', ど: 'do',
    な: 'na', に: 'ni', ぬ: 'nu', ね: 'ne', の: 'no',
    は: 'ha', ひ: 'hi', ふ: 'fu', へ: 'he', ほ: 'ho',
    ば: 'ba', び: 'bi', ぶ: 'bu', べ: 'be', ぼ: 'bo',
    ぱ: 'pa', ぴ: 'pi', ぷ: 'pu', ぺ: 'pe', ぽ: 'po',
    ま: 'ma', み: 'mi', む: 'mu', め: 'me', も: 'mo',
    や: 'ya', ゆ: 'yu', よ: 'yo', ら: 'ra', り: 'ri', る: 'ru', れ: 're', ろ: 'ro',
    わ: 'wa', を: 'wo', ん: 'n', ゃ: 'ya', ゅ: 'yu', ょ: 'yo', っ: '',
    きゃ: 'kya', きゅ: 'kyu', きょ: 'kyo', しゃ: 'sha', しゅ: 'shu', しょ: 'sho',
    ちゃ: 'cha', ちゅ: 'chu', ちょ: 'cho', にゃ: 'nya', にゅ: 'nyu', にょ: 'nyo',
    ひゃ: 'hya', ひゅ: 'hyu', ひょ: 'hyo', みゃ: 'mya', みゅ: 'myu', みょ: 'myo',
    りゃ: 'rya', りゅ: 'ryu', りょ: 'ryo', ぎゃ: 'gya', ぎゅ: 'gyu', ぎょ: 'gyo',
    じゃ: 'ja', じゅ: 'ju', じょ: 'jo', びゃ: 'bya', びゅ: 'byu', びょ: 'byo',
    ぴゃ: 'pya', ぴゅ: 'pyu', ぴょ: 'pyo', ' ': ' '
  };

  const isKana = $derived(/[\u3040-\u309f]/.test(text));
  const mode = $derived(isKana ? 'kana' : 'en');

  function codeFor(character: string): string | null {
    if (/^[a-z]$/i.test(character)) return `Key${character.toUpperCase()}`;
    if (/^[0-9]$/.test(character)) return `Digit${character}`;
    if (character === ' ') return 'Space';
    if (character === '\n') return 'Enter';
    if (character === '-') return 'Minus';
    return null;
  }

  async function press(code: string): Promise<void> {
    runtime.setKeyboardKey(code, true);
    await new Promise((resolve) => setTimeout(resolve, 20));
    runtime.setKeyboardKey(code, false);
  }

  async function sendText(value: string): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      const characters = Array.from(value);
      for (let index = 0; index < characters.length; index += 1) {
        const character = characters[index] ?? '';
        const combo = characters[index + 1] === undefined ? null : `${character}${characters[index + 1] ?? ''}`;
        const romaji = (combo !== null ? kanaToRomaji[combo] : undefined) ?? kanaToRomaji[character];
        if (romaji !== undefined) {
          if (combo !== null && kanaToRomaji[combo] !== undefined) index += 1;
          for (const letter of romaji) {
            const code = codeFor(letter);
            if (code !== null) await press(code);
          }
          continue;
        }
        const code = codeFor(character);
        if (code !== null) await press(code);
      }
    } finally {
      busy = false;
    }
  }

  function t(english: string, japanese: string): string {
    return language === 'en' ? english : japanese;
  }
</script>

<button
  type="button"
  class="rounded-lg bg-text/5 px-3 py-2 text-sm font-medium text-text hover:bg-text/10"
  onclick={() => (open = true)}
>{t('Keyboard', 'キーボード')}</button>

{#if open}
  <div class="fixed inset-0 z-50 grid place-items-center bg-black/60 p-4" role="presentation">
    <dialog
      open
      class="max-h-[85vh] w-full max-w-2xl overflow-auto rounded-xl border border-text/15 bg-surface1 p-5 shadow-2xl"
      aria-modal="true"
      aria-labelledby="software-keyboard-title"
    >
      <div class="flex items-center justify-between gap-3">
        <h2 id="software-keyboard-title" class="text-base font-semibold text-text">
          {t('Software keyboard', 'ソフトウェアキーボード')}
        </h2>
        <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-xs text-subtext1" onclick={() => (open = false)}>
          {t('Close', '閉じる')}
        </button>
      </div>

      <p class="mt-2 text-xs text-subtext0">
        {t('Mode', 'モード')}: <span class="font-mono text-text">{mode === 'kana' ? 'かな → ローマ字' : 'English'}</span>
      </p>

      <div class="mt-3 flex gap-2">
        <input
          class="min-w-0 flex-1 rounded-lg border border-text/15 bg-crust/40 px-3 py-2 font-mono text-sm text-text"
          aria-label={t('Text to send', '送信する文字列')}
          bind:value={text}
          placeholder={t('Type ASCII or kana, then Send', 'ASCIIまたはかなを入力して送信')}
        />
        <button
          type="button"
          class="rounded-lg bg-blue/15 px-3 py-2 text-sm text-blue disabled:opacity-40"
          disabled={busy || text.length === 0}
          onclick={() => void sendText(text)}
        >{t('Send', '送信')}</button>
      </div>

      {#if mode === 'en'}
        <div class="mt-4 space-y-1">
          {#each englishRows as row (row)}
            <div class="flex justify-center gap-1">
              {#each Array.from(row) as character (character)}
                <button type="button" class="size-9 rounded-md bg-text/10 font-mono text-sm text-text hover:bg-text/20" onclick={() => void sendText(character)}>{character.toUpperCase()}</button>
              {/each}
            </div>
          {/each}
          <div class="flex justify-center gap-1">
            <button type="button" class="rounded-md bg-text/10 px-4 py-2 text-xs text-text" onclick={() => void sendText(' ')}>Space</button>
            <button type="button" class="rounded-md bg-text/10 px-4 py-2 text-xs text-text" onclick={() => void sendText('\n')}>Enter</button>
          </div>
        </div>
      {:else}
        <div class="mt-4 space-y-1">
          {#each kanaRows as row (row)}
            <div class="flex justify-center gap-1">
              {#each Array.from(row) as character (character)}
                <button type="button" class="size-9 rounded-md bg-text/10 text-sm text-text hover:bg-text/20" onclick={() => void sendText(character)}>{character}</button>
              {/each}
            </div>
          {/each}
        </div>
      {/if}
    </dialog>
  </div>
{/if}
