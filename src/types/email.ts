export interface PaginatedEmailResponse {
  emails: EmailSummary[];
  total_count: number;
  current_page: number;
  total_pages: number;
}

export interface EmailSummary {
  id?: number;
  uid: number;
  message_id?: string;
  subject: string;
  from_address: string;
  from_name?: string;
  date: number;
  snippet?: string;
  has_attachments: boolean;
  is_read: boolean;
  account_email?: string;
}

export interface EmailBody {
  email_id: number;
  html_body?: string;
  text_body?: string;
}

export interface AttachmentInfo {
  id: string;
  filename: string;
  mime_type: string;
  size: number;
}
