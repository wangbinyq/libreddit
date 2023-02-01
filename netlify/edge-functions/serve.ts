import init, * as libreddit from '../../pkg/libreddit.js'

let inited = false;

export default async (req: Request) => {
  if (!inited) {
    await init('https://raw.githubusercontent.com/wangbinyq/libreddit/wasm/pkg/libreddit_bg.wasm')
    inited = true;
  }
  return libreddit.serve(req)
}
export const config = {
  path: '/*'
}