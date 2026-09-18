/** Synthetic catalog for an isolated, real Codex app-server compatibility probe. */
export function testCatalog() {
  return {
    models: [
      ['gptswitch/probe-a', '测试供应商 A · Code Model', 128000, ['low', 'high']],
      ['gptswitch/probe-b', '测试供应商 B · Vision Model', 64000, ['medium']],
    ].map(([slug, displayName, contextWindow, efforts], index) => ({
      slug,
      display_name: displayName,
      description: 'GPTSwitch isolated integration fixture; no real provider credentials.',
      default_reasoning_level: efforts[0],
      supported_reasoning_levels: efforts.map(effort => ({ effort, description: effort })),
      shell_type: 'unified_exec',
      visibility: 'list',
      supported_in_api: true,
      priority: index,
      availability_nux: null,
      upgrade: null,
      base_instructions: 'You are a coding assistant. Follow the user instructions.',
      support_verbosity: false,
      default_verbosity: null,
      apply_patch_tool_type: null,
      truncation_policy: { mode: 'tokens', limit: 10000 },
      context_window: contextWindow,
      max_context_window: contextWindow,
      effective_context_window_percent: 95,
      experimental_supported_tools: [],
      input_modalities: index === 0 ? ['text'] : ['text', 'image'],
      supports_reasoning_summary_parameter: false,
      supports_search_tool: false,
    })),
  };
}
