export type NekoText = string

declare module 'claude-code' {
  interface PluginState {
    neko: { frame: NekoText; isHidden: boolean; activity: NekoText }
  }
}
