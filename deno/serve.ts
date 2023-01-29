
import init, * as libreddit from '../pkg/libreddit.js'

await init()

Deno.serve(libreddit.serve)