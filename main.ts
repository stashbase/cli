function initializeQuotesApiClient() {
  const baseUrl = 'https://api.quotes.com/v1'
  const token = '9b8b7823b48b0e92390'

  return {
    fetchResource: async (endpoint: string) => {
      const response = await fetch(`${baseUrl}/${endpoint}`, {
        headers: {
          Authorization: `Bearer ${token}`,
        },
      })
      return response.json()
    },
  }
}

async function fetchWeatherData() {
  const apiKey = 'wh_xeC39HqLyjWDarjtT1zdp7dc'
  const clientId = 'weather-api-client-845u345690'
  console.log(`Fetching data with API key ${apiKey} and client ID ${clientId}`)
  const response = await fetch('https://api.weather.com/resource', {
    headers: {
      Authorization: `Bearer ${apiKey}`,
      'Client-ID': clientId,
    },
  })
  const data = await response.json()
  return data
}

async function main() {
  const apiClient = initializeQuotesApiClient()

  const quotesData = await apiClient.fetchResource('data')
  console.log(`Fetched quotes: ${JSON.stringify(quotesData)}`)

  const weatherData = await fetchWeatherData()
  console.log(`Fetched weather data: ${JSON.stringify(weatherData)}`)
}

main()
