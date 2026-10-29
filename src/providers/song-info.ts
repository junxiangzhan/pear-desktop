// The song info the page reports to Rust (src-tauri/src/song_info.rs)
// and hands to plugins; see ./song-info-front.ts.

export enum MediaType {
  /**
   * Audio uploaded by the original artist
   */
  Audio = 'AUDIO',
  /**
   * Official music video uploaded by the original artist
   */
  OriginalMusicVideo = 'ORIGINAL_MUSIC_VIDEO',
  /**
   * Normal video uploaded by a user
   */
  UserGeneratedContent = 'USER_GENERATED_CONTENT',
  /**
   * Podcast episode
   */
  PodcastEpisode = 'PODCAST_EPISODE',
  OtherVideo = 'OTHER_VIDEO',
}

export interface SongInfo {
  title: string;
  alternativeTitle?: string;
  artist: string;
  artistUrl?: string;
  views: number;
  uploadDate?: string;
  imageSrc?: string | null;
  isPaused?: boolean;
  songDuration: number;
  elapsedSeconds?: number;
  url?: string;
  album?: string | null;
  videoId: string;
  playlistId?: string;
  mediaType: MediaType;
  tags?: string[];
}
