import { atom, read, update } from 'claude-code'
import type { Hook, Register } from 'claude-code'

// neko 執行檔：有設 NEKO_BIN 就用它，不然用這個 repo 編出來的（mod/ 的上一層 target/release/neko）
let bin: string | undefined
async function nekoBin($: Engine): Promise<string> {
  if (bin) {
    return bin
  }
  const override = await $.env.get('NEKO_BIN')
  if (override) {
    return (bin = override)
  }
  const { CLAUDE_PLUGIN_ROOT: root = '' } = await $.process.hookEnv()
  const repo = root.replace(/\/+$/, '').replace(/\/[^/]+$/, '')
  return (bin = `${repo}/target/release/neko`)
}
const ACTS = ['eat', 'pet', 'play', 'call', 'jump', 'nap']
// 中文也能用：/neko 摸摸
const ALIASES: Record<string, string> = { 吃飯: 'eat', 摸摸: 'pet', 丟球: 'play', 叫牠: 'call', 跳: 'jump', 哄睡: 'nap', 收起: 'hide', 出來: 'show' }
// Claude 工作超過這麼久，貓就趴下睡；做完醒來叫一聲
const NAP_AFTER_MS = 45_000
// Claude 跑的 shell 指令超過這麼久還沒好，貓就睡著等；指令跑完醒來（貓只有這兩種時候會睡）
const SHELL_NAP_MS = 10_000
const MEOW_AFTER_MS = 20_000
const HELP = '/neko 乾糧、濕食、零食 餵食；吃飯、摸摸、丟球、叫牠、跳、哄睡、收起、出來（也可以用英文 dry、wet、treat、eat、pet、play、call、jump、nap、hide、show）；/neko 大小 1.5 調整解析度'

const frame = atom({ plugin: 'neko', key: 'frame' } as const, '')
const isHidden = atom({ plugin: 'neko', key: 'isHidden' } as const, false)
// 貓在看什麼（Claude 正在做的事）；空字串就顯示貓自己的狀態
const activity = atom({ plugin: 'neko', key: 'activity' } as const, '')

// Claude 正在用的工具 → 說明文字
function describe(e: { tool: string }): string {
  const fields = e as unknown as Record<string, unknown>
  const target = [fields.file_path, fields.notebook_path, fields.path, fields.pattern].find(x => typeof x === 'string') as string | undefined
  const file = target ? ` ${target.split('/').pop()}` : ''
  switch (e.tool) {
    case 'Read':
      return `盯著 Claude 讀${file}`
    case 'Grep':
    case 'Glob':
      return `陪 Claude 翻檔案找東西`
    case 'Edit':
    case 'Write':
    case 'NotebookEdit':
      return `看 Claude 改${file}`
    case 'Bash':
      return '看 Claude 跑指令'
    case 'Agent':
      return '看 Claude 找幫手'
    case 'WebSearch':
    case 'WebFetch':
      return '看 Claude 上網查資料'
    default:
      return `看 Claude 用 ${e.tool.replace(/^mcp__/, '').split('__').pop()}`
  }
}

// 放大倍率：字調小之後用 /neko 大小 1.5，同樣高度畫更多像素
let scale = 1

// 實際畫的倍率：輸入框上方的空間（maxRows）放不下 scale 倍的貓（11×倍率 列 + 1 列說明）就縮小，最小 0.4 倍
function fitScale(): number {
  const budget = live.maxRows - (live.maxRows >= 7 ? 1 : 0) // 太矮就連說明那一列都不要
  const k = Math.floor((budget / 11) * 100) / 100
  return Math.max(0.4, Math.min(scale, k))
}

async function act($: Engine, what: string, w: number) {
  if (await read($, isHidden)) {
    return ''
  }
  const { stdout } = await $.process.run([await nekoBin($), 'do', what, String(w), String(fitScale())], { timeoutMs: 2000 })

  return stdout.trim()
}

type Engine = Parameters<Hook<'session.start'>>[0]

// 餵食：/neko 乾糧、濕食、零食 → neko feed（Python 算營養、寫紀錄，貓演吃飯）
const FOODS: Record<string, string> = { 乾糧: 'dry', 濕食: 'wet', 零食: 'treat', dry: 'dry', wet: 'wet', treat: 'treat' }

async function feedCat($: Engine, food: string): Promise<string> {
  try {
    const r = await $.process.run([await nekoBin($), 'feed', food], { timeoutMs: 5000 })
    return (r.exitCode === 0 ? r.stdout : r.stderr.replace(/^neko: /, '')).trim()
  } catch {
    return '找不到 neko 執行檔'
  }
}

// 這一段工作期間的狀態（mod 重新載入時歸零）
const live = {
  width: 60,
  maxRows: 99, // 輸入框上方的空間能放幾列（第一次畫之前不知道）
  turnStartedAt: 0, // 主對話這一輪開始的時間；0 表示沒在跑
  hasNapped: false,
  bashSince: 0, // Claude 的 shell 指令從什麼時候開始跑；0 表示沒在跑
  bashNapped: false, // 現在睡著是因為在等 shell 指令（不是這一輪跑太久）
  lastStartleAt: 0,
  lastReactAt: 0,
  typingAt: 0,
}

// 貓的反應出錯不能擋到 Claude（送出訊息、跑工具）：錯誤吞掉，貓只是沒反應
async function quietly(f: () => unknown): Promise<void> {
  try {
    await f()
  } catch {
    // 貓沒反應而已
  }
}

async function setActivity($: Engine, text: string) {
  await update($, activity, () => text).catch(() => {})
}

// 自動的反應：貓睡著時不吵牠，兩次之間至少隔 gapMs
function react($: Engine, what: string, gapMs: number) {
  if (live.hasNapped || Date.now() - live.lastReactAt < gapMs) {
    return
  }
  live.lastReactAt = Date.now()
  void act($, what, live.width).catch(() => {})
}

// 畫面：一個一直跑的 neko stream 每秒送 15 格（每次叫一個新程式太慢，轉身等快動作會被跳過）。
// 寬度或倍率變了就重開。
async function streamFrames($: Engine) {
  let checkedDoze = false
  for (let attempt = 0; attempt < 10_000; attempt++) {
    // 收起來的時候不畫，等叫出來再開
    while (await read($, isHidden)) {
      await new Promise<void>(resolve => {
        $.clock.after(500, () => resolve())
      })
    }
    const started = `${live.width} ${fitScale()}`
    let buf = ''
    try {
      const child = $.process.spawn({ argv: [await nekoBin($), 'stream', String(live.width), String(fitScale()), '15'] })
      for await (const piece of child) {
        if (piece.stream !== 'stdout') {
          continue
        }
        buf += piece.text
        let cut = buf.indexOf('\x1e')
        while (cut >= 0) {
          const text = buf.slice(0, cut)
          buf = buf.slice(cut + 1)
          // 重開 Claude 或 mod 重新載入時，貓可能還停在上一次「等到睡著」（存在 pane.json，一直睡到 wake）：
          // 現在沒有在等 Claude，就叫醒牠
          if (!checkedDoze) {
            checkedDoze = true
            const caption = (text.split('\n')[1] ?? '').split('\t')[0] ?? ''
            if (caption.endsWith('等到睡著了') && live.turnStartedAt === 0 && !live.hasNapped) {
              void act($, 'wake', live.width).catch(() => {})
            }
          }
          if (text !== (await read($, frame))) {
            await update($, frame, () => text)
          }
          cut = buf.indexOf('\x1e')
        }
        if (`${live.width} ${fitScale()}` !== started || (await read($, isHidden))) {
          break // 離開迴圈會關掉舊的 stream
        }
      }
    } catch {
      await update($, frame, () => '找不到 neko 執行檔')
    }
    await new Promise<void>(resolve => {
      $.clock.after(300, () => resolve())
    })
  }
}

export const register: Register = on => {

  on('session.start', async ($, e, next) => {
    await $.command.register({
      name: 'neko',
      description: '輸入框上方的貓：乾糧、濕食、零食（餵食）、吃飯、摸摸、丟球、叫牠、跳、哄睡、收起、出來、大小',
      argumentHint: '[乾糧|濕食|零食|吃飯|摸摸|丟球|叫牠|跳|哄睡|收起|出來|大小 1.5]',
    })

    scale = Number((await $.store.get('scale')) ?? 1) || 1

    // 舊版開過的側邊面板：關掉
    void $.ui.close({ id: 'neko' }).catch(() => {})

    void streamFrames($)

    // 每半秒檢查：等太久要睡、停止打字要換回說明
    let isBusy = false
    $.clock.every(500, async () => {
      if (isBusy) {
        return
      }
      isBusy = true
      try {
        if (live.turnStartedAt > 0 && !live.hasNapped && Date.now() - live.turnStartedAt > NAP_AFTER_MS) {
          live.hasNapped = true
          await act($, 'doze', live.width) // 一直睡到這一輪結束
          await setActivity($, '等 Claude 等到睡著了')
        }
        if (live.bashSince > 0 && !live.hasNapped && Date.now() - live.bashSince > SHELL_NAP_MS) {
          live.hasNapped = true
          live.bashNapped = true
          await act($, 'doze', live.width) // 睡到指令跑完
          await setActivity($, '等指令跑完等到睡著了')
        }
        // 你停止打字 4 秒後，說明換回貓自己的狀態
        if (live.turnStartedAt === 0 && live.typingAt > 0 && Date.now() - live.typingAt > 4000) {
          live.typingAt = 0
          await setActivity($, '')
        }
      } catch {
        // 下一次再試
      } finally {
        isBusy = false
      }
    })

    return next(e)
  })

  on('command.run', { command: 'neko' }, async ($, e) => {
    const arg = e.args.trim()
    const sizeArg = /^(?:size|大小)\s*([\d.]+)?$/.exec(arg)
    if (sizeArg) {
      const wanted = Number(sizeArg[1])
      if (!(wanted >= 1 && wanted <= 4)) {
        return { text: `現在是 ${scale} 倍。用 /neko 大小 1.5 調整（1～4；字越小可以調越大，貓會畫得更細）。` }
      }
      scale = wanted
      await $.store.set('scale', scale)

      return { text: `貓改成 ${scale} 倍（${Math.ceil(11 * scale)} 列高）。` }
    }
    const food = FOODS[arg]
    if (food !== undefined) {
      await update($, isHidden, () => false)

      return { text: await feedCat($, food) }
    }
    const what = ALIASES[arg] ?? arg
    if (what === 'hide' || what === 'show') {
      await update($, isHidden, () => what === 'hide')

      return { text: what === 'hide' ? '貓收起來了。/neko show 叫牠出來。' : '貓出來了。' }
    }
    if (!ACTS.includes(what)) {
      return { text: HELP }
    }
    await update($, isHidden, () => false)

    return { text: await act($, what, live.width) }
  })

  // ── 貓跟著 Claude Code 反應 ──

  // 你在打字：貓看著你
  on('prompt.change', async ($, e, next) => {
    if (live.turnStartedAt === 0) {
      live.typingAt = e.text.trim() === '' ? 0 : Date.now()
      await setActivity($, e.text.trim() === '' ? '' : '看你打字')
    }

    return next(e)
  })

  // 你送出訊息：抬頭歪頭聽（斜線指令不算，免得蓋掉 /neko 的動作）
  on('prompt.submit', async ($, e, next) => {
    if (!e.text.trimStart().startsWith('/')) {
      await quietly(async () => {
        live.typingAt = 0
        live.lastReactAt = 0
        react($, 'look', 0)
        await setActivity($, '聽你說')
      })
    }

    return next(e)
  })

  on('turn.start', async ($, e, next) => {
    if (e.agentId === undefined) {
      live.turnStartedAt = Date.now()
    }

    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    if (e.agentId === undefined && live.turnStartedAt > 0) {
      const tookMs = Date.now() - live.turnStartedAt
      live.turnStartedAt = 0
      await setActivity($, '')
      // 等很久才做完、或貓已經睡著：叫一聲告訴你好了
      if (live.hasNapped) {
        // 睡著的貓：做完（或你中斷）時醒來、伸懶腰、叫一聲
        live.hasNapped = false
        live.bashNapped = false
        live.bashSince = 0
        void act($, 'wake', live.width).catch(() => {})
      } else if (!e.isAborted && tookMs > MEOW_AFTER_MS) {
        void act($, 'call', live.width).catch(() => {})
      }
    }

    return next(e)
  })

  // Claude 用工具：說明跟著換；改檔案時貓舉手洗臉陪著，指令失敗嚇一跳
  on('tool.call', async ($, e, next) => {
    if (e.agentId !== undefined) {
      return next(e)
    }
    await quietly(async () => {
      if (!live.hasNapped) {
        await setActivity($, describe(e))
      }
      if (e.tool === 'Bash') {
        live.bashSince = Date.now()
      }
    })
    let ran: Awaited<ReturnType<typeof next>>
    try {
      ran = await next(e)
    } finally {
      if (e.tool === 'Bash') {
        await quietly(async () => {
          live.bashSince = 0
          // 指令跑完：因為等指令睡著的就醒來；這一輪已經跑很久的話照樣睡到這一輪結束
          if (live.bashNapped) {
            live.bashNapped = false
            const turnLong = live.turnStartedAt > 0 && Date.now() - live.turnStartedAt > NAP_AFTER_MS
            if (!turnLong) {
              live.hasNapped = false
              void act($, 'wake', live.width).catch(() => {})
              await setActivity($, '')
            }
          }
        })
      }
    }
    const isOk = ran.deny === undefined && ran.isError !== true
    await quietly(async () => {
      if (!isOk && e.tool === 'Bash' && !live.hasNapped && Date.now() - live.lastStartleAt > 10_000) {
        live.lastStartleAt = Date.now()
        live.lastReactAt = Date.now()
        void act($, 'jump', live.width).catch(() => {})
        await setActivity($, '被失敗的指令嚇一跳')
      } else if (isOk && ['Edit', 'Write', 'NotebookEdit'].includes(e.tool)) {
        react($, 'paw', 8000)
      }
    })

    return ran
  })

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const text = await read($, frame)
    if (e.props.hasSurvey || text === '' || (await read($, isHidden))) {
      return next(e)
    }
    live.width = Math.max(40, Math.min(100 * scale, e.props.bodyColumns > 0 ? e.props.bodyColumns : (e.viewport?.columns ?? 68) - 8))
    live.maxRows = e.props.maxRows > 0 ? e.props.maxRows : 99
    if (e.surface !== 'terminal') {
      return next(e)
    }
    const { Box, Raster, Text } = $.ui.resolve(e)
    const [size = '', rustCaption = '', cells = ''] = text.split('\n')
    const doing = await read($, activity)
    // 「名字 行為<Tab>營養狀態」：Claude 在做事時行為換成那件事，營養狀態接在後面
    const [behavior = '', nutrition = ''] = rustCaption.split('\t')
    const said = doing === '' ? behavior : `${behavior.split(' ')[0]} ${doing}`
    const caption = nutrition === '' ? said : `${said}  ${nutrition}`
    const [columns = 0, rows = 0] = size.split(' ').map(Number)
    if (!(columns > 0 && rows > 0) || cells === '') {
      return next(e)
    }

    return (
      <Box flexDirection="column">
        <Raster key="cat" columns={columns} rows={rows} cells={cells} />
        {rows < live.maxRows ? (
          <Box paddingLeft={2}>
            <Text dimColor wrap="truncate-end">
              {caption}
            </Text>
          </Box>
        ) : null}
      </Box>
    )
  })
}
