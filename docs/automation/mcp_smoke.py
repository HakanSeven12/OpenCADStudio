"""Protocol smoke test for the bundled MCP adapter."""
import asyncio
from pathlib import Path
import sys
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

async def main() -> None:
    server = Path(__file__).with_name("mcp_server.py")
    params = StdioServerParameters(command=sys.executable, args=[str(server)])
    async with stdio_client(params) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            tools = await session.list_tools()
            names = {tool.name for tool in tools.tools}
            assert names == {"ocs_sessions", "ocs_read", "ocs_execute", "ocs_capture"}, names

if __name__ == "__main__":
    asyncio.run(main())
