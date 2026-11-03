/** Returns the id of the story that the file at `path` exports as `name`.
 *
 * `path` is relative to src/. The id is the path without `.story.tsx`,
 * then the export name. */
export const storyId = (path: string, name: string) =>
  `${path.replace(/\.story\.tsx$/, "")}/${name}`;
