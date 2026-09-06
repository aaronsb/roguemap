# Structures as block geometry

Buildings, roads, fields, walls and bridges are block geometry on the tile
grid, not sprites. A tile carries a stack: a block kind and a level count.
Stacking a block on a block makes it taller. Placing the same kind on
neighbouring tiles makes one larger building, the way Townscaper works.

## Data

`assets/blocks.toml` names each block kind:

| Field | Meaning |
|---|---|
| name | house, tower, barn, road, field, wall, bridge, dock, ... |
| level_height | height units per level (a house level is about 2) |
| roof | none, flat, gable, hip; gables ridge along the longer run |
| material | fixed, or by biome (the material table decides colours) |
| merge | whether same-kind neighbours share walls and roof |
| ground | how the tile beneath changes: flatten, pave, till |
| faces | glyph rows per level for walls: window bands, doors at ground on the open side |
| light | a light spec for lit windows at night |

A tile's structure is `Stack { kind, levels }`. Towns are painted by a
settlement system (near water, on flat ground, by biome) or by hand in
the editor: paint kinds on tiles, press again to stack.

## Geometry

The ray walk treats a stack as a column whose top is the terrain height
plus levels times level height. Above the terrain top the walk tests the
structure before the ground: the column's sides are walls with the block's
material and face glyphs, and the top is the roof. A roof is a height
profile inside the tile, so a gable rises to a ridge and the walk finds the
sloped surface at sub-tile resolution the same way it finds terrain detail.

Adjacency decides what is drawn. Where two merged tiles touch, the shared
wall is not a face and the roof profile continues across the seam. The
end walls of a run get the gable; the long sides get the eaves. A road
block has no height and paves the ground; a field block tills it; a bridge
block spans water at the bank height.

## Level of detail

At the overview a stack is its roof colour on a taller column, which the
existing renderer already draws. At mid zooms window bands and doors
appear as face glyphs. At the closest zooms props and creatures stand in
the streets. Nothing is placed per detail; the block kind and the
adjacency rules produce all of it.

## Order of work

1. The asset pass moves today's house sprite and material rules into
   tables; the block table starts with house, road and field.
2. The rasteriser gains the structure test above the terrain top and
   flat roofs, then gable and hip profiles.
3. Adjacency merging and face glyph bands.
4. The settlement system paints towns; the editor paints by hand.

## Trees as volumes

The same walk renders trees as geometry. A species names a trunk column
and a canopy shape: a cone for conifers, an ellipsoid for broadleaf, a low
dome for scrub, a column with arms for cactus, with a size class. A ray at
a given height tests the canopies whose footprint covers its ground point,
so a tree occludes correctly at every angle, crowns in a dense stand merge
into one mass, and the crown's normal shades it toward the sun. Glyph
texture goes on the surface the walk finds, so the foliage character of
each glyph set survives. An L-system species produces branch and leaf
volumes for the walk instead of rows of characters.

At the overview a tree stays a one-glyph column. Billboards remain for
creatures and small props, which face the camera anyway.
