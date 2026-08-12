let editorNavigationStartedAt: number | null = null;

export const markNavigationStart = (path: string) => {
  if (path !== "/editor") return;
  editorNavigationStartedAt = performance.now();
  performance.mark("editor-navigation-start");
};

export const markEditorReady = () => {
  if (editorNavigationStartedAt === null) return;
  const elapsed = performance.now() - editorNavigationStartedAt;
  document.documentElement.dataset.editorNavigationMs = elapsed.toFixed(1);
  performance.mark("editor-navigation-ready");
  performance.measure("editor-navigation", "editor-navigation-start", "editor-navigation-ready");
  editorNavigationStartedAt = null;
};
