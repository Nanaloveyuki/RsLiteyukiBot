import key from '@/const/key';

export function applyWebUiAppearanceToStorage (appearance: WebUIAppearanceState) {
  localStorage.setItem(key.backgroundImage, appearance.backgroundImage ?? '');
  localStorage.setItem(key.customIcons, JSON.stringify(appearance.customIcons ?? {}));
}
