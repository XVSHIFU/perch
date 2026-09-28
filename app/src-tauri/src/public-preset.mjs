const levels = new Set(['off', 'low', 'high', 'max']);
const conflict = () => {
  const error = Error('Public preset conflicts with workbench configuration');
  error.code = 'PERCH_PRESET_CONFLICT';
  throw error;
};

// Only a public, enumerated preference is managed here. Keep its previous value
// in the same settings document so existing snapshots restore both atomically.
export function mergeThinkingPreset(current, owned, next) {
  if (next == null) next = undefined;
  if (next !== undefined && !levels.has(next)) conflict();
  if (owned !== undefined && (!owned || typeof owned !== 'object' ||
      !levels.has(owned.value) || Object.keys(owned).some(key => !['value', 'previous'].includes(key)) ||
      (owned.previous !== undefined && typeof owned.previous !== 'string'))) conflict();
  if (next === undefined) {
    // Removing the preset relinquishes control. Preserve intervening manual edits.
    return {value: owned && current === owned.value ? owned.previous : current};
  }
  if (current !== undefined && typeof current !== 'string') conflict();
  if (current !== undefined && current !== next && (!owned || current !== owned.value)) conflict();
  if (owned && current === undefined) conflict();
  return {value: next, owned: {value: next, ...(owned?.previous !== undefined
    ? {previous: owned.previous} : !owned && current !== undefined ? {previous: current} : {})}};
}

export function applyDshThinkingPreset(doc, next) {
  const preset = mergeThinkingPreset(doc.getIn(['agent-default-model', 'reasoningEffort']),
    doc.toJS()?.['perch-thinking-preset'], next);
  // A fresh instance may not have agent-default-model yet. YAML deleteIn
  // throws on a missing intermediate mapping; absence needs no mutation.
  if (preset.value === undefined) {
    if (doc.hasIn(['agent-default-model', 'reasoningEffort'])) doc.deleteIn(['agent-default-model', 'reasoningEffort']);
  }
  else doc.setIn(['agent-default-model', 'reasoningEffort'], preset.value);
  if (preset.owned === undefined) {
    if (doc.hasIn(['perch-thinking-preset'])) doc.deleteIn(['perch-thinking-preset']);
  }
  else doc.setIn(['perch-thinking-preset'], preset.owned);
}
