import pluginIconFallbacks from '@/config/plugin_icon_fallbacks.json';

type AvatarColor = 'default' | 'primary' | 'secondary' | 'success' | 'warning' | 'danger';

interface PluginIconInput {
  id?: string;
  name?: string;
  author?: string;
  tags?: string[];
  sourceKind?: string;
  compatKind?: string;
  pluginType?: string;
  runtimeKind?: string;
  homepage?: string;
  repository?: string;
  downloadUrl?: string;
}

interface PluginIconFallbackRule {
  label: string;
  color: AvatarColor;
  match: string[];
}

const fallbackRules = pluginIconFallbacks as Record<string, PluginIconFallbackRule>;
const fallbackRuleList = Object.values(fallbackRules)
  .sort((left, right) => {
    const leftMaxLength = Math.max(...left.match.map((matcher) => matcher.length));
    const rightMaxLength = Math.max(...right.match.map((matcher) => matcher.length));
    return rightMaxLength - leftMaxLength;
  });

function collectSearchText (plugin: PluginIconInput) {
  return [
    plugin.id,
    plugin.name,
    plugin.author,
    plugin.sourceKind,
    plugin.compatKind,
    plugin.pluginType,
    plugin.runtimeKind,
    plugin.homepage,
    plugin.repository,
    plugin.downloadUrl,
    ...(plugin.tags ?? []),
  ]
    .filter(Boolean)
    .join(' ')
    .toLowerCase();
}

function firstNameLetter (name?: string) {
  return name?.trim().charAt(0).toUpperCase() || 'L';
}

export function resolvePluginIconFallback (plugin: PluginIconInput) {
  const searchText = collectSearchText(plugin);

  for (const rule of fallbackRuleList) {
    if (rule.match.some((matcher) => searchText.includes(matcher.toLowerCase()))) {
      return {
        label: rule.label,
        color: rule.color,
      };
    }
  }

  return {
    label: firstNameLetter(plugin.name || plugin.id),
    color: 'default' as AvatarColor,
  };
}
