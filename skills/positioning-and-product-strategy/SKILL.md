---
name: positioning-and-product-strategy
description: "Position a product from what buyers would use instead, its unique attributes, their value and the best-fit customer, before messaging; brand architecture. Use for positioning or a vision review."
authorized_agents:
  - strategist
input_schema:
  type: object
  required: ["product"]
  properties:
    product:
      type: string
      description: "The product, feature set or offer to position, in the operator's words"
    documents:
      type: array
      items: {type: string}
      description: "Paths to the vision, the strategy and the roadmap, when the review of step 7 is wanted"
    evidence:
      type: array
      items: {type: string}
      description: "Paths or sources about buyers and what they use today"
output_schema:
  type: object
  required: ["positioning"]
  properties:
    positioning:
      type: string
      description: "The positioning page in the fixed layout, as text or as the path it was written to"
    review:
      type: string
      description: "The vision to strategy to roadmap findings, when documents were given"
required_permissions:
  - read:all
  - write:missions
---
# Positioning and Product Strategy

Positioning says what a product is, who it is for and why it is the better choice for them. It is worked out before any message is written: a tagline cannot fix a product that is compared with the wrong thing.

## When to use
- The operator asks how to position a product, who it is for, or what to call the category it is in.
- Messaging, a landing page or a launch is planned and no positioning page exists yet.
- Several products or names need an order: which carries the brand, which stands alone.
- A vision, a strategy and a roadmap exist and someone asks whether they agree.

## The order
Each part rests on the one before it. Do not start at the bottom.

1. **Competitive alternatives:** what the best-fit buyer would do if this product did not exist. That includes a spreadsheet, a person hired to do it, and doing nothing.
2. **Unique attributes:** what this product has that those alternatives do not. A feature the alternatives also have is not on this list.
3. **Value:** what each attribute lets the buyer do, in the buyer's words, with the proof you can point at.
4. **Best-fit customer:** the kind of buyer who cares most about that value, by traits you could check (size, situation, what they tried before), not "everyone".
5. **Market category:** the frame that makes the value obvious to that buyer. Pick the frame last.
6. **Messaging:** only now, and outside this skill. Hand the page to the writer.

## Layout
```markdown
# Positioning: <product>

## Competitive alternatives
- <alternative>: <source, or "from model memory, not sourced">

## Unique attributes
- <attribute> (not in: <which alternatives>)

## Value
- <attribute> lets <buyer> <do what>. Proof: <where it can be seen, or "none yet">

## Best-fit customer
- <checkable traits>

## Market category
- <category>, because <why this frame makes the value obvious>

## Not for
- <who should not buy this, and what they should use instead>
```

## Steps
1. Read what the workspace holds on the product and its buyers before writing anything. A claim about a buyer, a competitor or a market carries its source, or the label `from model memory, not sourced`; ask the scout when the claim matters and no source is at hand.
2. List the competitive alternatives from the buyer's side. If you only list products that look like this one, you have listed competitors, not alternatives: add what buyers do today.
3. List the unique attributes, each against the alternatives that lack it. Strike any attribute you cannot show.
4. Turn each attribute into value, with its proof. An attribute with no value to the best-fit buyer is cut from the page, however hard it was to build.
5. Name the best-fit customer and write the "Not for" line. A page with no "Not for" has not chosen.
6. Choose the market category, and say in one sentence what the buyer will assume about the product because of it.
7. **Brand architecture, when more than one product or name is involved.** Say which arrangement is meant and why: one brand over everything, separate brands that do not mention each other, a brand that endorses the others, or a main brand with named sub-products. Then say what a buyer of one will assume about the others, and whether that assumption helps.
8. **Vision to strategy to roadmap review, when the documents are given.** Read them in that order and report:
   - a vision that names no buyer and no change in that buyer's situation;
   - a strategy that does not say what is in the way of the vision or what is ruled out (use `strategy-kernel` to judge it);
   - a roadmap item that leads to no choice in the strategy;
   - **a roadmap that is a list of features**: items with no outcome, no buyer and no link to the strategy. Say so in those words, name the items, and say what each would need to become a step (the outcome it is for and how it would be seen).
9. Report the page, the one part you are least sure of, and what evidence would settle it.

## Done when
The page has all six sections in the layout, in the order above; every alternative and every proof carries a source or the label `from model memory, not sourced`; "Not for" names someone; no message, tagline or slogan is on the page; and, where documents were given, the review names each roadmap item that leads to no strategy choice, or says there is none.

## Pitfalls
- Starting from the message and working back to a customer who would like it.
- Positioning against the product you admire instead of what the buyer uses now.
- Attributes that are true of every alternative ("easy to use", "fast").
- A best-fit customer drawn so wide that it rules nobody out.
- Calling a roadmap strategic because its features are grouped under themes. A theme is a label, not a choice.
- Treating this page as evidence. It is a set of choices and claims; the claims are only as good as their sources.

## Sources
The order (competitive alternatives, unique attributes, value, best-fit customer, market category) is commonly attributed to April Dunford, and the brand architecture arrangements to David Aaker and Erich Joachimsthaler. This text was written from secondary summaries, not from the primary works, and has not been checked against them. It cites no primary work until the operator ratifies the sources (`specs/124` §12, OD-3).
