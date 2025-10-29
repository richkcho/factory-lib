# factory-lib
Rust libraries for modeling factory game components. Currently deeply a WIP. I *really* want to build a factory game simulation that can scale very hard. I'm going to be more likely to make tradeoffs that prefer scaability as long as things aren't too unexpected, where the expectations are mine. 

# Design assumptions
TBH I don't really know what I'm doing. But I like the satisfactory style format that prefers splitters (Satisfactory) over inserters (Factorio, DSP). It seems like this should make scalability and optimizations easier, but who knows. So for now I'm going to have splitters/mergers and no inserters.

Some principles I'd like to hold:
* Ability to run the game forward multiple ticks at a time: batch updates if possible in a way that reduces CPU usage. Instead of running forward the simulation forward 1 tick, can we run 2 ticks in less than 2x the time?
* Don't troll perf: things may not be super optimized from the get go, but if I see simple operations taking O(n) that seem like they shouldn't or O(n^2) when it should be O(n), I'm going to fix that. 
* Lazy eval? Or some type of only updating parts of the factory that are actually doing something in a intelligent fashion. 

I need to figure out push vs pull and what all the apis look like for factory entities later. 

### Notes:
#### 1. Splitters and tick batching
So I think all entities (including splitters) should consume from internal buffer and output onto belts. 
I think this implies we will always have buffers in everything in order to allow the "Ability to run the game forward multiple ticks at a time." At least, it makes that problem significantly easier. 
The main problem is splitters + round robin. I'm tempted to just call it a day in terms of splitters batch size == tick batch size and if you want better performance you just need to accept that. Reasonable tradeoff? Not sure ig. 