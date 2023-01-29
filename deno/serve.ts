import { serve } from 'https://deno.land/std@0.175.0/http/server.ts'
import init, * as libreddit from '../pkg/libreddit.js'

await init()

serve(libreddit.serve)