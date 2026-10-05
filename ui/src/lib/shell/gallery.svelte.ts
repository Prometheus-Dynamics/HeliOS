// The gallery dialog: add screens, kits of panes, or single panes.
class GalleryStore {
  open = $state(false);
  tab = $state<"screens" | "kits" | "panes">("screens");

  show(tab: GalleryStore["tab"]) {
    this.tab = tab;
    this.open = true;
  }
}

export const gallery = new GalleryStore();
