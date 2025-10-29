# factory-lib
Rust libraries for modeling factory game components. Currently deeply a WIP. I *really* want to build a factory game simulation that can scale very hard. I think the interesting "mechanic" here is to be able to compile a factory segment - this incentivises modular design as well as rendering many other performance optimizations obselete or far less important. Also it's cool. 

# Design assumptions
TBH I don't really know what I'm doing. But I like the satisfactory style format that prefers splitters (Satisfactory) over inserters (Factorio, DSP). It seems like this should make scalability and optimizations easier, but who knows. So for now I'm going to have splitters/mergers and no inserters.

Where possible, prefer batching processing. This is significantly easier / possible to do if there are buffers. 