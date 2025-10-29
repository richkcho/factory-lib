# factory-lib
Rust libraries for modeling factory game components

# Design assumptions
TBH I don't really know what I'm doing. But I like the satisfactory style format that prefers splitters (Satisfactory) over inserters (Factorio, DSP). It seems like this should make scalability and optimizations easier, but who knows. So for now I'm going to have splitters/mergers and no inserters.

Some principles I'd like to hold:
* Ability to run the game forward multiple ticks at a time: batch updates if possible in a way that reduces CPU usage. 
* Don't troll perf: things may not be super optimized from the get go, but if I see simple operations taking O(n) that seem like they shouldn't or O(n^2) when it should be O(n), I'm going to fix that. 

I need to figure out push vs pull and what all the apis look like for factory entities later. 