# Desktop PDF export

PDF generation runs on a separate native worker from an immutable document snapshot, retaining
its document fonts until completion. The interface shows an indeterminate working indicator and
elapsed time and continues processing window events. Editing and normal Quit wait until the job
finishes, so colour settings and document state cannot change halfway through the output. The
desktop host also writes the completed bytes atomically on that worker (custom hosts without a thread-safe writer save on completion); errors and export warnings are shown explicitly.
Only one PDF export runs at a time. Periodic recovery writes wait until it completes, as they did
while the synchronous export blocked the interface. If the operating system forces the app to
exit, its final recovery save still preserves unsaved documents while export is pending.
Browser export remains synchronous.

Files delivered by an open dialog or the operating system wait in their queues while exporting.
Dropped graphics retain their original pointer position and are placed once the job finishes.
Normal Quit is remembered and resumes after success or failure, still asking about unsaved
documents. Forced automation shutdown (`app.quit` on the control channel) can interrupt a job.

The native worker uses the same export pipeline as `file.exportPdf`, returning raw PDF bytes
instead of creating and decoding a large base64 JSON copy. Original artwork, output options and
PDF quality are unchanged. `ui.inspect.pdfExport` reports `busy` and the last completed result.
Programmatic `app.exportPdf` returns `{path, queued:true}` on desktop; inspect the job for final
success or failure. The engine's `file.exportPdf` remains synchronous for scripts.

Background jobs retain the caller’s automation roots. Output is checked on the worker and again before a custom host performs a deferred write, so moving export off the UI thread does not broaden filesystem access.

When a caller grants several read or write roots, the worker and deferred writer retain the complete set. Each output must lie in a granted write root; their common parent and read-only roots remain refused.
