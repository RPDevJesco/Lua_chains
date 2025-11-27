return {
  context = {
    counter = 0,
    message = "start"
  },
  events = {
    {
      name = "increment",
      handler = function(ctx)
        ctx.counter = ctx.counter + 1
        return ctx
      end
    },
    {
      name = "append",
      handler = function(ctx)
        ctx.message = ctx.message .. " -> processed"
        return ctx
      end
    }
  },
  middleware = {
    {
      name = "timing",
      handler = function(ctx, next)
        -- Just pass through for now
        return next(ctx)
      end
    }
  }
}
