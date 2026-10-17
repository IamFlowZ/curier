export type ViewMode = 'feeds' | 'list';

export interface AppSettings {
  sync_interval_seconds: number;
  enable_background_sync: boolean;
  max_headers_per_folder: number;
  max_bodies_total: number;
  emails_per_page: number;
  approved_image_domains: string[];
  /** Default view mode per inbox: key is account email or "all", value is "feeds" or "list" */
  default_view_mode: Record<string, ViewMode>;
  updated_at: number;
}
