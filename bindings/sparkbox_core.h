/*
 * sparkbox_core.h — C ABI of the SparkBoxCore FFI library (sparkbox-ffi).
 *
 * Every function returns a NUL-terminated UTF-8 JSON string allocated by the
 * library. Free it with sparkbox_free_string(). All functions are BLOCKING
 * (they perform network I/O); call them off the UI thread / main isolate.
 *
 * Envelope:
 *   success: {"ok":true,  "data": ...}
 *   failure: {"ok":false, "error":"..."}
 *
 * Payload shapes (camelCase, same keys as the Flutter app's models):
 *   MediaItem    {id,title,isSeries,detailPath,cover,backdrop,rating,year,
 *                 genre,country,description,durationMin,subjectType}
 *   HomeSection  {title,items:[MediaItem],ranked,isBanner}
 *   Detail       {id,title,detailPath,h5Base,h5Token,note}
 *   Season       {se,maxEp,episodes:[int]}
 *   StreamLink   {label,quality,url,kind:"hls"|"dash"|"mp4",headers:{...}}
 *   SubtitleLink {label,url}
 *
 * Build: cargo build --release -p sparkbox-ffi
 *   Android/Linux: libsparkbox_ffi.so · macOS: libsparkbox_ffi.dylib
 *   Windows: sparkbox_ffi.dll      · iOS/static: libsparkbox_ffi.a
 */

#ifndef SPARKBOX_CORE_H
#define SPARKBOX_CORE_H

#ifdef __cplusplus
extern "C" {
#endif

/* {"ok":true,"data":{"version":"0.1.0","blockAdult":true}} */
char *sparkbox_core_version(void);

/* {"ok":true,"data":{"blockAdult":bool}} — 1 = hide adult content */
char *sparkbox_set_block_adult(int blocked);

/* data: [HomeSection] — home feed (mobile first, h5 fallback, + Trending). */
char *sparkbox_home(void);

/* data: [string] — as-you-type keyword suggestions. */
char *sparkbox_suggest(const char *query);

/* data: {"items":[MediaItem],"hasMore":bool} — rich paginated search. */
char *sparkbox_search(const char *query, int page, int per_page);

/* arg: MediaItem JSON · data: {"item":MediaItem,"dubs":[{name,subjectId,
 * detailPath}],"cast":[{name,avatarUrl}]} */
char *sparkbox_detail_rich(const char *item_json);

/* arg: MediaItem JSON · data: Detail — feed straight into sparkbox_streams. */
char *sparkbox_play_detail(const char *item_json);

/* data: [Season] — episodes of a series. */
char *sparkbox_seasons(const char *subject_id);

/* args: Detail JSON, season, episode, is_series(0/1)
 * data: {"links":[StreamLink],"subtitles":[SubtitleLink]} */
char *sparkbox_streams(const char *detail_json, int se, int ep, int is_series);

/* data: {"label":"MOVIE"|"SERIES"|"VIDEO"|"MEDIA"} */
char *sparkbox_type_label(int subject_type);

/* data: ["hls","dash","mp4"] — the strings used in StreamLink.kind. */
char *sparkbox_stream_kinds(void);

/* Free any string returned above. NULL is a no-op. */
void sparkbox_free_string(char *ptr);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* SPARKBOX_CORE_H */
